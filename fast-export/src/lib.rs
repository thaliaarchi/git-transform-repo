//! Library for generating `git fast-export`–format streams, to export data as a
//! repository.

mod ast;
mod pretty;

pub use ast::*;
pub use pretty::*;
