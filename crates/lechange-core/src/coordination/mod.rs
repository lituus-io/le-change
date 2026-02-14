//! Main coordination logic

pub mod ci_decision;
pub mod processor;
pub mod workflow_tracker;

pub use ci_decision::CiDecisionEngine;
pub use processor::FileProcessor;
pub use workflow_tracker::WorkflowTracker;
