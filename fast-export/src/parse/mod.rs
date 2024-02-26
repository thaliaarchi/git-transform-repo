mod data;
mod parser;

pub use data::*;
pub use parser::*;

pub(super) type Result<T> = std::result::Result<T, StreamError>;
