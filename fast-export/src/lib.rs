//! Library for generating `git fast-export`–format streams, to export data as a
//! repository.

pub mod ast;
mod pretty;

pub use pretty::Pretty;
