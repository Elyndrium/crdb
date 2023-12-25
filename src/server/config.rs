// This module needs to actually be public, because `generate_server!` needs to be
// able to implement Config. However, making it doc(hidden) makes it look as though
// it is actually sealed in the documentation, which is good because we don't want
// users to rely on any stability guarantees there.
#[doc(hidden)]
pub mod private {
    #[doc(hidden)]
    pub trait Sealed {}
}

/// Note: Implementation of this trait is supposed to be provided by `crdb::db!`
pub trait Config: private::Sealed {
    type Auth;
}

#[doc(hidden)]
#[macro_export]
macro_rules! generate_server {
    ( $auth:ty | $api_config:ident | $name:ident | $($object:ty),* ) => {
        pub struct $name;

        impl server::config::private::Sealed for $name {}
        impl server::Config for $name {
            type Auth = $auth;
        }
    };
}
