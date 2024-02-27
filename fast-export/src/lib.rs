// Copyright (C) Thalia Archibald. All rights reserved.
//
// This file is part of fast-export-rust, distributed under the GPL 2.0 with a
// linking exception. For the full terms, see the included COPYING file.

//! Library for generating `git fast-export`–format streams, to export data as a
//! repository.

mod bytes;
pub mod command;
mod dump;
pub mod parse;

pub use bytes::FromBytes;
pub use dump::Dump;
