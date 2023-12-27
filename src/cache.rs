use crate::{
    api::{BinPtr, Query},
    db_trait::{Db, EventId, NewEvent, NewObject, NewSnapshot, ObjectId, Timestamp},
    full_object::FullObject,
    hash_binary, Object, User,
};
use anyhow::Context;
use futures::{pin_mut, Stream, StreamExt};
use std::sync::Arc;
use tokio::sync::RwLock;

mod binaries_cache;
mod config;
mod object_cache;

pub use binaries_cache::BinariesCache;
pub use config::CacheConfig;
pub use object_cache::ObjectCache;

pub(crate) struct Cache<D: Db> {
    db: Arc<D>,
    // TODO: figure out how to purge from cache (LRU-style), using DeepSizeOf
    cache: Arc<RwLock<ObjectCache>>,
    binaries: Arc<RwLock<BinariesCache>>,
}

impl<D: Db> Cache<D> {
    fn watch_from<C: CacheConfig, OtherDb: Db>(&self, db: &Arc<OtherDb>, relay_to_db: bool) {
        // Watch new objects
        tokio::task::spawn({
            let db = db.clone();
            let internal_db = self.db.clone();
            let cache = self.cache.clone();
            async move {
                let new_objects = db.new_objects().await;
                pin_mut!(new_objects);
                while let Some(o) = new_objects.next().await {
                    let mut cache = cache.write().await;
                    let object = o.id;
                    if relay_to_db {
                        if let Err(error) = C::create_in_db(&*internal_db, o.clone()).await {
                            tracing::error!(
                                ?error,
                                ?object,
                                "failed creating received object in internal db"
                            );
                        }
                    }
                    if let Err(error) = C::create(&mut *cache, o).await {
                        tracing::error!(
                            ?error,
                            ?object,
                            "failed creating received object in cache"
                        );
                    }
                }
            }
        });

        // Watch new events
        tokio::task::spawn({
            let db = db.clone();
            let internal_db = self.db.clone();
            let cache = self.cache.clone();
            async move {
                let new_events = db.new_events().await;
                pin_mut!(new_events);
                while let Some(e) = new_events.next().await {
                    let mut cache = cache.write().await;
                    let object = e.object_id;
                    let event = e.id;
                    if relay_to_db {
                        if let Err(error) = C::submit_in_db(&*internal_db, e.clone()).await {
                            tracing::error!(
                                ?error,
                                ?object,
                                ?event,
                                "failed submitting received object to internal db"
                            );
                        }
                    }
                    // DO NOT re-fetch object when receiving an event not in cache for it.
                    // Without this, users would risk unsubscribing from an object, then receiving
                    // an event on this object (as a race condition), and then staying subscribed.
                    if let Err(error) = C::submit::<D>(None, &mut *cache, e).await {
                        tracing::error!(
                            ?error,
                            ?object,
                            ?event,
                            "failed submitting received event to cache"
                        );
                    }
                }
            }
        });

        // Watch new snapshots
        tokio::task::spawn({
            let db = db.clone();
            let internal_db = self.db.clone();
            let cache = self.cache.clone();
            async move {
                let new_snapshots = db.new_snapshots().await;
                pin_mut!(new_snapshots);
                while let Some(s) = new_snapshots.next().await {
                    let mut cache = cache.write().await;
                    let object = s.object_id;
                    if relay_to_db {
                        if let Err(error) = C::snapshot_in_db(&*internal_db, s.clone()).await {
                            tracing::error!(
                                ?error,
                                ?object,
                                "failed snapshotting as per received event in internal db"
                            );
                        }
                    }
                    if let Err(error) = C::snapshot(&mut *cache, s).await {
                        tracing::error!(
                            ?error,
                            ?object,
                            "failed snapshotting as per received event in cache"
                        )
                    }
                }
            }
        });
    }

    pub(crate) fn new<C: CacheConfig>(db: Arc<D>) -> Cache<D> {
        let cache = Arc::new(RwLock::new(ObjectCache::new()));
        let this = Cache {
            db: db.clone(),
            cache,
            binaries: Arc::new(RwLock::new(BinariesCache::new())),
        };
        this.watch_from::<C, _>(&db, false);
        this
    }

    /// Relays all new objects/events from `db` to the internal database, caching them in the process.
    pub(crate) fn also_watch_from<C: CacheConfig, OtherDb: Db>(&self, db: &Arc<OtherDb>) {
        self.watch_from::<C, _>(db, true)
    }

    pub(crate) async fn clear_cache(&self) {
        self.clear_binaries_cache().await;
        self.clear_objects_cache().await;
    }

    pub(crate) async fn clear_binaries_cache(&self) {
        self.binaries.write().await.clear();
    }

    pub(crate) async fn clear_objects_cache(&self) {
        self.cache.write().await.clear();
    }
}

impl<D: Db> Db for Cache<D> {
    async fn new_objects(&self) -> impl Stream<Item = NewObject> {
        self.db.new_objects().await
    }

    async fn new_events(&self) -> impl Stream<Item = NewEvent> {
        self.db.new_events().await
    }

    async fn new_snapshots(&self) -> impl Stream<Item = NewSnapshot> {
        self.db.new_snapshots().await
    }

    async fn unsubscribe(&self, ptr: ObjectId) -> anyhow::Result<()> {
        let mut cache = self.cache.write().await;
        cache.remove(&ptr).await;
        self.db.unsubscribe(ptr).await
    }

    async fn create<T: Object>(
        &self,
        id: ObjectId,
        created_at: EventId,
        object: Arc<T>,
    ) -> anyhow::Result<()> {
        let mut cache = self.cache.write().await;
        if cache.create(id, created_at, object.clone()).await? {
            self.db.create(id, created_at, object).await?;
        }
        Ok(())
    }

    async fn submit<T: Object>(
        &self,
        object_id: ObjectId,
        event_id: EventId,
        event: Arc<T::Event>,
    ) -> anyhow::Result<()> {
        let mut cache = self.cache.write().await;
        if cache
            .submit::<D, T>(Some(&*self.db), object_id, event_id, event.clone())
            .await?
        {
            self.db.submit::<T>(object_id, event_id, event).await?;
        }
        Ok(())
    }

    async fn get<T: Object>(&self, ptr: ObjectId) -> anyhow::Result<Option<FullObject>> {
        {
            let cache = self.cache.read().await;
            if let Some(res) = cache.get(&ptr) {
                return Ok(Some(res.clone()));
            }
        }
        let Some(res) = self.db.get::<T>(ptr).await? else {
            return Ok(None);
        };
        debug_assert!(
            res.id() == ptr,
            "Got result with id {:?} instead of expected id {ptr:?}",
            res.id()
        );
        {
            let mut cache = self.cache.write().await;
            cache
                .insert::<T>(res.clone())
                .await
                .with_context(|| format!("inserting object {ptr:?} in the cache"))?;
        }
        Ok(Some(res))
    }

    async fn query<T: Object>(
        &self,
        user: User,
        include_heavy: bool,
        ignore_not_modified_on_server_since: Option<Timestamp>,
        q: Query,
    ) -> anyhow::Result<impl Stream<Item = anyhow::Result<FullObject>>> {
        // We cannot use the object cache here, because it is not guaranteed to even
        // contain all the non-heavy objects, due to being an LRU cache. So, immediately
        // delegate to the underlying database, which should forward to either PostgreSQL
        // for the server, or IndexedDB or the API for the client, depending on whether
        // `include_heavy` is set.
        Ok(self
            .db
            .query::<T>(user, include_heavy, ignore_not_modified_on_server_since, q)
            .await?
            .then(|o| async {
                let o = o?;
                let mut cache = self.cache.write().await;
                if let Err(error) = cache.insert::<T>(o.clone()).await {
                    let id = o.id();
                    tracing::error!(?id, ?error, "failed inserting queried object in cache");
                    cache.remove(&id).await;
                }
                Ok(o)
            }))
    }

    async fn snapshot<T: Object>(&self, time: Timestamp, object: ObjectId) -> anyhow::Result<()> {
        let mut cache = self.cache.write().await;
        cache.snapshot::<T>(object, time).await?;
        self.db.snapshot::<T>(time, object).await
    }

    async fn create_binary(&self, id: BinPtr, value: Arc<Vec<u8>>) -> anyhow::Result<()> {
        debug_assert!(
            id == hash_binary(&*value),
            "Provided id {id:?} does not match value hash {:?}",
            hash_binary(&*value),
        );
        self.binaries.write().await.insert(id, value.clone());
        self.db.create_binary(id, value).await
    }

    async fn get_binary(&self, ptr: BinPtr) -> anyhow::Result<Option<Arc<Vec<u8>>>> {
        if let Some(res) = self.binaries.read().await.get(&ptr) {
            return Ok(Some(res.clone()));
        }
        let Some(res) = self.db.get_binary(ptr).await? else {
            return Ok(None);
        };
        self.binaries.write().await.insert(ptr, res.clone());
        Ok(Some(res))
    }
}
