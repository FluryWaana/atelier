pub mod application;
pub mod contracts;
pub mod document;
pub mod domain;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;
