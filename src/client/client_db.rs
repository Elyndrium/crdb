use super::{ApiDb, Authenticator, IndexedDb};
use crate::{
    api,
    cache::Cache,
    db_trait::{Db, EventId, FullObject, NewEvent, NewObject, NewSnapshot, ObjectId},
    BinPtr, Object, Query, Timestamp, User,
};
use anyhow::anyhow;
use futures::Stream;
use std::sync::Arc;

#[doc(hidden)]
pub struct ClientDb<A: Authenticator> {
    api: ApiDb<A>,
    db: Cache<IndexedDb>,
}

impl<A: Authenticator> ClientDb<A> {
    pub async fn connect<C: api::Config>(
        base_url: Arc<String>,
        auth: Arc<A>,
    ) -> anyhow::Result<ClientDb<A>> {
        Ok(ClientDb {
            api: ApiDb::connect(base_url, auth).await?,
            db: Cache::new::<C>(Arc::new(IndexedDb::new())),
        })
    }

    pub async fn disconnect(self) -> anyhow::Result<()> {
        self.api.disconnect().await
    }
}

#[allow(unused_variables)] // TODO: remove
impl<A: Authenticator> Db for ClientDb<A> {
    async fn new_objects(&self) -> impl Stream<Item = NewObject> {
        self.api.new_objects().await
    }

    async fn new_events(&self) -> impl Send + Stream<Item = NewEvent> {
        self.api.new_events().await
    }

    async fn new_snapshots(&self) -> impl Send + Stream<Item = NewSnapshot> {
        self.api.new_snapshots().await
    }

    async fn unsubscribe(&self, ptr: ObjectId) -> anyhow::Result<()> {
        self.api.unsubscribe(ptr).await
    }

    async fn create<T: Object>(
        &self,
        id: ObjectId,
        created_at: EventId,
        object: Arc<T>,
    ) -> anyhow::Result<()> {
        self.api.create(id, created_at, object.clone()).await?;
        self.db.create(id, created_at, object).await
    }

    async fn submit<T: Object>(
        &self,
        object: ObjectId,
        event_id: EventId,
        event: Arc<T::Event>,
    ) -> anyhow::Result<()> {
        self.api
            .submit::<T>(object, event_id, event.clone())
            .await?;
        self.db.submit::<T>(object, event_id, event).await
    }

    async fn get<T: Object>(&self, ptr: ObjectId) -> anyhow::Result<Option<FullObject>> {
        if let Some(res) = self.db.get::<T>(ptr).await? {
            return Ok(Some(res));
        }
        let Some(o) = self.api.get::<T>(ptr).await? else {
            return Ok(None);
        };
        self.db
            .create::<T>(
                o.id,
                o.created_at,
                o.creation
                    .clone()
                    .downcast::<T>()
                    .map_err(|_| anyhow!("API returned object of unexpected type"))?,
            )
            .await?;
        for (event_id, c) in o.changes.iter() {
            self.db
                .submit::<T>(
                    o.id,
                    *event_id,
                    c.event
                        .clone()
                        .downcast::<T::Event>()
                        .map_err(|_| anyhow!("API returned object of unexpected type"))?,
                )
                .await?;
        }
        Ok(Some(o))
    }

    async fn query<T: Object>(
        &self,
        user: User,
        include_heavy: bool,
        ignore_not_modified_on_server_since: Option<Timestamp>,
        q: Query,
    ) -> anyhow::Result<impl Stream<Item = FullObject>> {
        // todo!()
        Ok(futures::stream::empty())
    }

    async fn snapshot<T: Object>(&self, time: Timestamp, object: ObjectId) -> anyhow::Result<()> {
        todo!()
    }

    async fn create_binary(&self, id: BinPtr, value: Arc<Vec<u8>>) -> anyhow::Result<()> {
        todo!()
    }

    async fn get_binary(&self, ptr: BinPtr) -> anyhow::Result<Arc<Vec<u8>>> {
        todo!()
    }
}
