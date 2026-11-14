//! Meetings: the recorder, its audio kept on disk, rolling digests and
//! the action plan, the end of a meeting, the notepad's AI helpers, the
//! refined transcript, the meeting notepad window, the disk space a
//! recording has left, the PC going to sleep mid-meeting, and meetings
//! imported from other apps' transcript files.
//! How it works: docs/architecture/meetings.md.

pub mod coach_marks;
pub mod commands;
pub mod disk_watch;
pub mod import;
pub mod recorder;
pub mod assist;
pub mod audio;
pub mod end;
pub mod refine;
pub mod summary;
pub mod notepad;
pub mod sleep;
