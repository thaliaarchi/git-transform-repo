//! Library for generating `git fast-export`–format streams, to export data as a
//! repository.

pub mod command;
mod dump;

pub use dump::Dump;
