// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

mod data;
mod parser;

pub use data::*;
pub use parser::*;

pub(super) type PResult<T> = Result<T, StreamError>;
