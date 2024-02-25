//! Library for generating `git fast-export`–format streams, to export data as a
//! repository.

mod bytes;
pub mod command;
mod dump;
pub mod parse;

pub use bytes::FromBytes;
pub use dump::Dump;
