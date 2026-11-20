//! SCIP indexers: which one indexes a language, running it in a project
//! root (progress, timeout, actionable errors), deciding when an existing
//! `index.scip` can be reused, and caching its documents per file content so
//! other checkouts of the repository can skip the indexer. Ported from
//! Structure's pipeline.

mod cache;
mod custom;
mod ensure;
mod git;
mod indexers;
mod process;
mod progress;

pub use cache::{Coverage, DocumentCache, FragmentCache};
pub use custom::{CustomIndex, ENV_VAR as INDEX_COMMAND_ENV};
pub use ensure::{
    DEFAULT_TIMEOUT_SECS, IndexOptions, IndexSource, Reindex, default_timeout, ensure_index,
    newer_sources,
};
pub use git::hash_paths as blob_ids;
pub use indexers::{
    DEFAULT_NODE_HEAP_MB, Indexer, SCIP_JAVA_VERSION, SCIP_TS_VERSION, discover_tsconfigs,
    for_language, indexers, node_options, ts_index_args,
};
pub use process::run;
