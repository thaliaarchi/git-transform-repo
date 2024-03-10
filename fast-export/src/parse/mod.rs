// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

mod data;
mod input;
mod parser;
mod pool;

pub use data::*;
pub(self) use input::*;
pub use parser::*;
pub(self) use pool::*;

pub(crate) type PResult<T> = Result<T, StreamError>;
