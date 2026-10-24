//! Meetings: the recorder, its audio kept on disk, rolling digests and
//! the action plan, the end of a meeting, the notepad's AI helpers, the
//! refined transcript and the meeting notepad window.
//! How it works: docs/architecture/meetings.md.

pub mod meeting;
pub mod meeting_assist;
pub mod meeting_audio;
pub mod meeting_end;
pub mod meeting_refine;
pub mod meeting_summary;
pub mod notepad;
