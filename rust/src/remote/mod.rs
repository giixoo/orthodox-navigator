//! Remote panels: folders on other machines over SFTP (the system `sftp`) and FTP / FTPS (the system
//! `curl`). See docs/superpowers/specs/2026-11-19-remote-panels-design.md.

// Parts are wired into the panels step by step; remove once all of it is used (plan Task 8).
#![allow(dead_code)]

pub mod askpass;
pub mod listing;
pub mod profiles;
pub mod vault;
pub mod wait;
