mod data;
mod parser;

pub use data::*;
pub use parser::*;

pub(super) type PResult<T> = Result<T, StreamError>;
