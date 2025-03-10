// Copyright 2022 RisingLight Project Authors. Licensed under Apache-2.0.

//! RisingLight -- an educational OLAP database.

#![warn(clippy::doc_markdown)]
#![warn(clippy::explicit_into_iter_loop)]
#![warn(clippy::explicit_iter_loop)]
#![warn(clippy::inconsistent_struct_constructor)]
#![warn(clippy::map_flatten)]
#![deny(unused_must_use)]
#![feature(portable_simd)]
#![feature(generators)]
#![feature(backtrace)]
#![feature(generic_associated_types)]
#![feature(type_alias_impl_trait)]
#![feature(stmt_expr_attributes)]
#![feature(proc_macro_hygiene)]
#![feature(core_intrinsics)]

/// Top-level structure of the database.
pub mod db;

/// Stage 1: Parse the SQL string into an Abstract Syntax Tree (AST).
pub mod parser;

/// Stage 2: Resolve all expressions referring with their names.
pub mod binder;

/// Stage 3: Transform the parse tree into a logical operations tree.
pub mod logical_planner;

/// Stage 4: Do query optimization.
pub mod optimizer;

/// Stage 5: Execute the queries.
pub mod executor;

/// In-memory representations of a column values.
pub mod array;
/// Metadata of database objects.
pub mod catalog;
/// Persistent storage engine.
pub mod storage;
/// Basic type definitions.
pub mod types;
/// Utilities.
pub mod utils;

#[cfg(feature = "jemalloc")]
use tikv_jemallocator::Jemalloc;

pub use self::db::{Database, Error};

/// Jemalloc can significantly improve performance compared to the default system allocator.
#[cfg(feature = "jemalloc")]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;
