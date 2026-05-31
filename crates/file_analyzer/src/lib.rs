pub mod scanner;
pub mod chunker;
pub mod classifier;
pub mod imports;
pub mod git_context;

pub use scanner::{FileInfo, ProjectScanner};
pub use chunker::Chunker;
pub use classifier::{TaskClassifier, TaskCategory};
pub use imports::{parse_imports, resolve_deps, build_project_tree};
pub use git_context::{GitContext, git_context};
