//! FBX: the reader's node tree (`tree`), the scene of the committed exports
//! (`scene`), the object graph on synthetic files (`graph`), and the format
//! through the SDK's contract (`format`, `measure`), and hostile files
//! (`hostile`).
mod common;
mod format;
mod graph;
mod hostile;
mod measure;
mod scene;
mod tree;
