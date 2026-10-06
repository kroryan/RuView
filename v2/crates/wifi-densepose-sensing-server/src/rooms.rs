//! Room registry: named groups of ESP32 nodes that are calibrated, monitored
//! and displayed together.
//!
//! A *room* is the operator's unit of work: "these nodes sit around the same
//! physical space". Calibration is bound to a room (its id **and** its exact
//! node set, so adding or removing a node invalidates the old baseline), the
//! live aggregates (presence, person count, vitals) are computed only from the
//! room's nodes, and the UIs let the user pick which room they are looking at.
//!
//! The registry is small, validated, persisted atomically as JSON in the data
//! directory, and never contains CSI or personal data — only names and node
//! ids. A node may belong to at most one room.
//!
//! This module also holds the pure link-health evaluation used to decide
//! whether a room's nodes are working together well enough (matching frame
//! rates, low packet loss, comparable CSI grids, alive links).

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const ROOMS_SCHEMA: &str = "ruview.rooms.v1";
pub const MAX_ROOMS: usize = 16;
pub const MAX_NODES_PER_ROOM: usize = 16;
pub const MAX_NAME_CHARS: usize = 48;
const MAX_FILE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RoomError {
    #[error("room name must be 1 to {MAX_NAME_CHARS} printable characters")]
    InvalidName,
    #[error("a room needs 1 to {MAX_NODES_PER_ROOM} distinct node ids")]
    InvalidNodes,
    #[error("a room with this name already exists")]
    DuplicateName,
    #[error("node {0} already belongs to another room")]
    NodeInOtherRoom(u8),
    #[error("at most {MAX_ROOMS} rooms are supported")]
    TooManyRooms,
    #[error("unknown room")]
    NotFound,
    #[error("invalid room id")]
    InvalidId,
}

impl RoomError {
    pub fn code(&self) -> &'static str {
        match self {
            RoomError::InvalidName => "room_name_invalid",
            RoomError::InvalidNodes => "room_nodes_invalid",
            RoomError::DuplicateName => "room_name_duplicate",
            RoomError::NodeInOtherRoom(_) => "room_node_conflict",
            RoomError::TooManyRooms => "room_limit_reached",
            RoomError::NotFound => "room_not_found",
            RoomError::InvalidId => "room_id_invalid",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub id: String,
    pub name: String,
    /// Sorted, unique logical ESP32 node ids.
    pub node_ids: Vec<u8>,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

impl Room {
    pub fn contains(&self, node_id: u8) -> bool {
        self.node_ids.binary_search(&node_id).is_ok()
    }

    pub fn node_set(&self) -> BTreeSet<u8> {
        self.node_ids.iter().copied().collect()
    }

    /// Lowercase SHA-256 binding for calibration of exactly this room and
    /// node set. Any membership change produces a different digest.
    pub fn binding_digest(&self) -> String {
        let nodes: Vec<String> = self.node_ids.iter().map(u8::to_string).collect();
        let material = format!("ruview-room-v1|{}|{}", self.id, nodes.join("-"));
        Sha256::digest(material.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RoomRegistry {
    pub schema: String,
    pub rooms: Vec<Room>,
    pub active_room_id: Option<String>,
}

impl Default for RoomRegistry {
    fn default() -> Self {
        Self {
            schema: ROOMS_SCHEMA.to_string(),
            rooms: Vec::new(),
            active_room_id: None,
        }
    }
}

pub fn valid_room_id(id: &str) -> bool {
    id.len() == 13
        && id.starts_with("room-")
        && id[5..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn normalise_name(name: &str) -> Result<String, RoomError> {
    let trimmed = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars = trimmed.chars().count();
    if chars == 0 || chars > MAX_NAME_CHARS || trimmed.chars().any(char::is_control) {
        return Err(RoomError::InvalidName);
    }
    Ok(trimmed)
}

fn normalise_nodes(node_ids: &[u8]) -> Result<Vec<u8>, RoomError> {
    let set: BTreeSet<u8> = node_ids.iter().copied().collect();
    if set.is_empty() || set.len() > MAX_NODES_PER_ROOM {
        return Err(RoomError::InvalidNodes);
    }
    Ok(set.into_iter().collect())
}

fn new_room_id() -> String {
    let mut bytes = [0_u8; 4];
    OsRng.fill_bytes(&mut bytes);
    format!(
        "room-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

impl RoomRegistry {
    pub fn room(&self, id: &str) -> Option<&Room> {
        self.rooms.iter().find(|room| room.id == id)
    }

    pub fn active_room(&self) -> Option<&Room> {
        self.active_room_id.as_deref().and_then(|id| self.room(id))
    }

    pub fn room_of_node(&self, node_id: u8) -> Option<&Room> {
        self.rooms.iter().find(|room| room.contains(node_id))
    }

    /// Node ids that live aggregates must be restricted to, or `None` when no
    /// room is active (legacy behaviour: every node participates).
    pub fn active_member_filter(&self) -> Option<BTreeSet<u8>> {
        self.active_room().map(Room::node_set)
    }

    fn check_nodes_free(&self, nodes: &[u8], except_room: Option<&str>) -> Result<(), RoomError> {
        for &node in nodes {
            if let Some(owner) = self.room_of_node(node) {
                if Some(owner.id.as_str()) != except_room {
                    return Err(RoomError::NodeInOtherRoom(node));
                }
            }
        }
        Ok(())
    }

    fn check_name_free(&self, name: &str, except_room: Option<&str>) -> Result<(), RoomError> {
        let lower = name.to_lowercase();
        if self
            .rooms
            .iter()
            .any(|r| Some(r.id.as_str()) != except_room && r.name.to_lowercase() == lower)
        {
            return Err(RoomError::DuplicateName);
        }
        Ok(())
    }

    pub fn create(&mut self, name: &str, node_ids: &[u8], now_ms: u64) -> Result<Room, RoomError> {
        if self.rooms.len() >= MAX_ROOMS {
            return Err(RoomError::TooManyRooms);
        }
        let name = normalise_name(name)?;
        let nodes = normalise_nodes(node_ids)?;
        self.check_name_free(&name, None)?;
        self.check_nodes_free(&nodes, None)?;
        let mut id = new_room_id();
        while self.room(&id).is_some() {
            id = new_room_id();
        }
        let room = Room {
            id,
            name,
            node_ids: nodes,
            created_at_unix_ms: now_ms,
            updated_at_unix_ms: now_ms,
        };
        self.rooms.push(room.clone());
        Ok(room)
    }

    /// Update name and/or membership. Returns the updated room and whether the
    /// node set changed (which invalidates any calibration bound to it).
    pub fn update(
        &mut self,
        id: &str,
        name: Option<&str>,
        node_ids: Option<&[u8]>,
        now_ms: u64,
    ) -> Result<(Room, bool), RoomError> {
        if !valid_room_id(id) {
            return Err(RoomError::InvalidId);
        }
        let current = self.room(id).ok_or(RoomError::NotFound)?.clone();
        let new_name = match name {
            Some(n) => {
                let n = normalise_name(n)?;
                self.check_name_free(&n, Some(id))?;
                n
            }
            None => current.name.clone(),
        };
        let new_nodes = match node_ids {
            Some(nodes) => {
                let nodes = normalise_nodes(nodes)?;
                self.check_nodes_free(&nodes, Some(id))?;
                nodes
            }
            None => current.node_ids.clone(),
        };
        let nodes_changed = new_nodes != current.node_ids;
        let room = self
            .rooms
            .iter_mut()
            .find(|room| room.id == id)
            .ok_or(RoomError::NotFound)?;
        room.name = new_name;
        room.node_ids = new_nodes;
        room.updated_at_unix_ms = now_ms;
        Ok((room.clone(), nodes_changed))
    }

    pub fn delete(&mut self, id: &str) -> Result<Room, RoomError> {
        if !valid_room_id(id) {
            return Err(RoomError::InvalidId);
        }
        let position = self
            .rooms
            .iter()
            .position(|room| room.id == id)
            .ok_or(RoomError::NotFound)?;
        let removed = self.rooms.remove(position);
        if self.active_room_id.as_deref() == Some(id) {
            self.active_room_id = None;
        }
        Ok(removed)
    }

    pub fn set_active(&mut self, id: Option<&str>) -> Result<(), RoomError> {
        match id {
            None => {
                self.active_room_id = None;
                Ok(())
            }
            Some(id) => {
                if !valid_room_id(id) {
                    return Err(RoomError::InvalidId);
                }
                self.room(id).ok_or(RoomError::NotFound)?;
                self.active_room_id = Some(id.to_string());
                Ok(())
            }
        }
    }

    /// Structural validation after loading untrusted JSON.
    fn validate(&self) -> bool {
        if self.schema != ROOMS_SCHEMA || self.rooms.len() > MAX_ROOMS {
            return false;
        }
        let mut seen_nodes = BTreeSet::new();
        let mut seen_ids = BTreeSet::new();
        let mut seen_names = BTreeSet::new();
        for room in &self.rooms {
            if !valid_room_id(&room.id)
                || !seen_ids.insert(room.id.clone())
                || normalise_name(&room.name).ok().as_deref() != Some(room.name.as_str())
                || !seen_names.insert(room.name.to_lowercase())
                || room.node_ids.is_empty()
                || room.node_ids.len() > MAX_NODES_PER_ROOM
                || room.node_ids.windows(2).any(|w| w[0] >= w[1])
                || room.node_ids.iter().any(|n| !seen_nodes.insert(*n))
            {
                return false;
            }
        }
        match &self.active_room_id {
            Some(active) => self.room(active).is_some(),
            None => true,
        }
    }
}

pub fn path_in(data_dir: &Path) -> PathBuf {
    data_dir.join("rooms.json")
}

/// Load the registry. A missing file yields an empty registry; a corrupt or
/// inconsistent file is rejected (returned as `Err`) so the caller can log it
/// and continue with an empty registry rather than trusting bad state.
pub fn load(path: &Path) -> Result<RoomRegistry, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RoomRegistry::default())
        }
        Err(error) => return Err(format!("cannot stat rooms file: {error}")),
    };
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err("rooms file is not a regular file or is too large".to_string());
    }
    let bytes = fs::read(path).map_err(|error| format!("cannot read rooms file: {error}"))?;
    let registry: RoomRegistry =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid rooms file: {error}"))?;
    if !registry.validate() {
        return Err("rooms file failed validation".to_string());
    }
    Ok(registry)
}

/// Atomically persist the registry (write a sibling temp file, fsync, rename).
pub fn save(path: &Path, registry: &RoomRegistry) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| format!("cannot create data dir: {error}"))?;
    let json = serde_json::to_vec_pretty(registry)
        .map_err(|error| format!("cannot encode rooms: {error}"))?;
    let tmp = parent.join(format!(".rooms.json.{}.tmp", std::process::id()));
    {
        let mut file = fs::File::create(&tmp)
            .map_err(|error| format!("cannot create temporary rooms file: {error}"))?;
        file.write_all(&json)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("cannot write rooms file: {error}"))?;
    }
    fs::rename(&tmp, path).map_err(|error| {
        let _ = fs::remove_file(&tmp);
        format!("cannot replace rooms file: {error}")
    })
}

// ── Link health ────────────────────────────────────────────────────────────

/// What the server knows about one node's link.
#[derive(Debug, Clone, Default)]
pub struct NodeLink {
    pub node_id: u8,
    /// A fresh CSI frame arrived recently.
    pub online: bool,
    pub fps: Option<f64>,
    pub last_seen_age_ms: Option<u64>,
    /// Decayed fraction of CSI packets missing according to the sequence counter.
    pub loss_ratio: Option<f64>,
    pub grid: Option<(u16, u8)>,
    pub mesh_sync_valid: Option<bool>,
    pub mesh_sync_staleness_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HealthIssue {
    pub code: &'static str,
    pub severity: &'static str,
    pub node_ids: Vec<u8>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoomHealth {
    /// `empty`, `offline`, `degraded` or `aligned`.
    pub verdict: &'static str,
    pub nodes_total: usize,
    pub nodes_online: usize,
    pub fps_min: Option<f64>,
    pub fps_max: Option<f64>,
    pub max_loss_ratio: Option<f64>,
    pub issues: Vec<HealthIssue>,
}

pub const RATE_MISMATCH_RATIO: f64 = 1.25;
pub const LOSS_WARN_RATIO: f64 = 0.10;
pub const HEART_MIN_FPS: f64 = 8.0;
pub const MESH_SYNC_STALE_MS: u64 = 9_000;

/// Evaluate whether the nodes of one room are working together.
pub fn evaluate_room_health(links: &[NodeLink]) -> RoomHealth {
    let nodes_total = links.len();
    if nodes_total == 0 {
        return RoomHealth {
            verdict: "empty",
            nodes_total,
            nodes_online: 0,
            fps_min: None,
            fps_max: None,
            max_loss_ratio: None,
            issues: Vec::new(),
        };
    }
    let online: Vec<&NodeLink> = links.iter().filter(|l| l.online).collect();
    let mut issues = Vec::new();

    let offline: Vec<u8> = links.iter().filter(|l| !l.online).map(|l| l.node_id).collect();
    if !offline.is_empty() {
        issues.push(HealthIssue {
            code: "node_offline",
            severity: "error",
            message: format!("No fresh CSI from node(s) {}.", join_ids(&offline)),
            node_ids: offline,
        });
    }

    let rates: Vec<(u8, f64)> = online
        .iter()
        .filter_map(|l| l.fps.filter(|f| f.is_finite() && *f > 0.0).map(|f| (l.node_id, f)))
        .collect();
    let fps_min = rates.iter().map(|r| r.1).fold(None, |a: Option<f64>, v| {
        Some(a.map_or(v, |x| x.min(v)))
    });
    let fps_max = rates.iter().map(|r| r.1).fold(None, |a: Option<f64>, v| {
        Some(a.map_or(v, |x| x.max(v)))
    });
    if let (Some(lo), Some(hi)) = (fps_min, fps_max) {
        if hi / lo > RATE_MISMATCH_RATIO {
            let slow: Vec<u8> = rates
                .iter()
                .filter(|(_, f)| *f * RATE_MISMATCH_RATIO < hi)
                .map(|r| r.0)
                .collect();
            issues.push(HealthIssue {
                code: "rate_mismatch",
                severity: "warning",
                message: format!(
                    "Nodes sample at different rates ({lo:.1} to {hi:.1} fps). Fusion keeps working, \
                     but match the firmware CSI rate on node(s) {}.",
                    join_ids(&slow)
                ),
                node_ids: slow,
            });
        }
        let low: Vec<u8> = rates
            .iter()
            .filter(|(_, f)| *f < HEART_MIN_FPS)
            .map(|r| r.0)
            .collect();
        if !low.is_empty() {
            issues.push(HealthIssue {
                code: "low_rate",
                severity: "warning",
                message: format!(
                    "Node(s) {} run below {HEART_MIN_FPS:.0} fps; heart-rate analysis needs a faster CSI stream.",
                    join_ids(&low)
                ),
                node_ids: low,
            });
        }
    }

    let lossy: Vec<u8> = online
        .iter()
        .filter(|l| l.loss_ratio.is_some_and(|r| r > LOSS_WARN_RATIO))
        .map(|l| l.node_id)
        .collect();
    if !lossy.is_empty() {
        issues.push(HealthIssue {
            code: "packet_loss",
            severity: "warning",
            message: format!(
                "Node(s) {} lose more than {:.0}% of CSI packets. Check Wi-Fi congestion and distance to the access point.",
                join_ids(&lossy),
                LOSS_WARN_RATIO * 100.0
            ),
            node_ids: lossy,
        });
    }
    let max_loss_ratio = online
        .iter()
        .filter_map(|l| l.loss_ratio)
        .fold(None, |a: Option<f64>, v| Some(a.map_or(v, |x| x.max(v))));

    let grids: BTreeSet<(u16, u8)> = online.iter().filter_map(|l| l.grid).collect();
    if grids.len() > 1 {
        issues.push(HealthIssue {
            code: "grid_mismatch",
            severity: "info",
            node_ids: online.iter().filter(|l| l.grid.is_some()).map(|l| l.node_id).collect(),
            message: "Nodes report different CSI subcarrier layouts; they are analysed per node and never mixed bin by bin."
                .to_string(),
        });
    }

    let unsynced: Vec<u8> = online
        .iter()
        .filter(|l| {
            l.mesh_sync_valid == Some(false)
                || l.mesh_sync_staleness_ms.is_some_and(|ms| ms > MESH_SYNC_STALE_MS)
        })
        .map(|l| l.node_id)
        .collect();
    if !unsynced.is_empty() {
        issues.push(HealthIssue {
            code: "mesh_sync_lost",
            severity: "warning",
            message: format!(
                "Node(s) {} lost firmware mesh time sync; the server aligns them by arrival time instead.",
                join_ids(&unsynced)
            ),
            node_ids: unsynced,
        });
    }
    if online.len() > 1 && online.iter().all(|l| l.mesh_sync_valid.is_none()) {
        issues.push(HealthIssue {
            code: "mesh_sync_absent",
            severity: "info",
            node_ids: online.iter().map(|l| l.node_id).collect(),
            message: "No firmware mesh sync packets seen; nodes are aligned by server arrival time (valid for breathing, approximate for fine fusion).".to_string(),
        });
    }

    let verdict = if online.is_empty() {
        "offline"
    } else if issues.iter().any(|i| i.severity != "info") {
        "degraded"
    } else {
        "aligned"
    };
    RoomHealth {
        verdict,
        nodes_total,
        nodes_online: online.len(),
        fps_min,
        fps_max,
        max_loss_ratio,
        issues,
    }
}

fn join_ids(ids: &[u8]) -> String {
    ids.iter().map(u8::to_string).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with_two() -> RoomRegistry {
        let mut registry = RoomRegistry::default();
        registry.create("Salón", &[3, 1, 2, 2], 10).unwrap();
        registry.create("Dormitorio", &[7], 11).unwrap();
        registry
    }

    #[test]
    fn creates_sorted_unique_rooms_with_opaque_ids() {
        let registry = registry_with_two();
        let room = &registry.rooms[0];
        assert_eq!(room.node_ids, vec![1, 2, 3]);
        assert!(valid_room_id(&room.id), "{}", room.id);
        assert_ne!(registry.rooms[0].id, registry.rooms[1].id);
    }

    #[test]
    fn a_node_belongs_to_one_room_and_names_are_unique() {
        let mut registry = registry_with_two();
        assert_eq!(
            registry.create("Cocina", &[3, 9], 12),
            Err(RoomError::NodeInOtherRoom(3))
        );
        assert_eq!(
            registry.create("  salón ", &[9], 12),
            Err(RoomError::DuplicateName)
        );
        assert_eq!(registry.create("   ", &[9], 12), Err(RoomError::InvalidName));
        assert_eq!(registry.create("X", &[], 12), Err(RoomError::InvalidNodes));
        let many: Vec<u8> = (100..120).collect();
        assert_eq!(registry.create("Big", &many, 12), Err(RoomError::InvalidNodes));
    }

    #[test]
    fn membership_change_changes_the_calibration_binding() {
        let mut registry = registry_with_two();
        let id = registry.rooms[0].id.clone();
        let before = registry.room(&id).unwrap().binding_digest();
        assert_eq!(before.len(), 64);
        let (room, changed) = registry.update(&id, None, Some(&[1, 2, 3, 4]), 20).unwrap();
        assert!(changed);
        assert_ne!(before, room.binding_digest());
        let (_, changed) = registry.update(&id, Some("Sala"), None, 21).unwrap();
        assert!(!changed, "renaming alone must keep the calibration valid");
        assert_eq!(
            registry.room(&id).unwrap().binding_digest(),
            room.binding_digest()
        );
    }

    #[test]
    fn a_node_can_move_after_it_is_released() {
        let mut registry = registry_with_two();
        let a = registry.rooms[0].id.clone();
        let b = registry.rooms[1].id.clone();
        assert_eq!(
            registry.update(&b, None, Some(&[7, 3]), 30).unwrap_err(),
            RoomError::NodeInOtherRoom(3)
        );
        registry.update(&a, None, Some(&[1, 2]), 31).unwrap();
        registry.update(&b, None, Some(&[7, 3]), 32).unwrap();
        assert_eq!(registry.room_of_node(3).unwrap().id, b);
    }

    #[test]
    fn active_room_is_validated_and_cleared_on_delete() {
        let mut registry = registry_with_two();
        let id = registry.rooms[0].id.clone();
        assert_eq!(registry.set_active(Some("room-zzzz")), Err(RoomError::InvalidId));
        assert_eq!(
            registry.set_active(Some("room-00000000")),
            Err(RoomError::NotFound)
        );
        registry.set_active(Some(&id)).unwrap();
        assert_eq!(registry.active_member_filter().unwrap().len(), 3);
        registry.delete(&id).unwrap();
        assert!(registry.active_room_id.is_none());
        assert!(registry.active_member_filter().is_none());
    }

    #[test]
    fn persistence_round_trips_and_rejects_tampering() {
        let dir = std::env::temp_dir().join(format!("ruview-rooms-{}", std::process::id()));
        let path = path_in(&dir);
        let mut registry = registry_with_two();
        let id = registry.rooms[1].id.clone();
        registry.set_active(Some(&id)).unwrap();
        save(&path, &registry).unwrap();
        assert_eq!(load(&path).unwrap(), registry);

        // Duplicate node across rooms must be rejected on load.
        let mut bad = registry.clone();
        bad.rooms[1].node_ids = vec![1];
        fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(load(&path).is_err());
        // Unknown field is rejected.
        fs::write(&path, br#"{"schema":"ruview.rooms.v1","rooms":[],"active_room_id":null,"x":1}"#).unwrap();
        assert!(load(&path).is_err());
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(load(&path).unwrap(), RoomRegistry::default());
    }

    fn link(id: u8, fps: f64) -> NodeLink {
        NodeLink {
            node_id: id,
            online: true,
            fps: Some(fps),
            loss_ratio: Some(0.0),
            grid: Some((64, 0)),
            mesh_sync_valid: Some(true),
            mesh_sync_staleness_ms: Some(500),
            last_seen_age_ms: Some(30),
        }
    }

    #[test]
    fn matching_nodes_are_aligned() {
        let health = evaluate_room_health(&[link(1, 20.0), link(2, 19.0), link(3, 21.0)]);
        assert_eq!(health.verdict, "aligned");
        assert!(health.issues.is_empty(), "{:?}", health.issues);
        assert_eq!(health.nodes_online, 3);
    }

    #[test]
    fn mismatched_lossy_offline_nodes_are_reported() {
        let mut slow = link(2, 6.0);
        slow.loss_ratio = Some(0.3);
        let mut dead = link(3, 20.0);
        dead.online = false;
        let health = evaluate_room_health(&[link(1, 20.0), slow, dead]);
        assert_eq!(health.verdict, "degraded");
        let codes: Vec<&str> = health.issues.iter().map(|i| i.code).collect();
        for expected in ["node_offline", "rate_mismatch", "low_rate", "packet_loss"] {
            assert!(codes.contains(&expected), "{expected} missing in {codes:?}");
        }
    }

    #[test]
    fn empty_and_offline_rooms_have_their_own_verdicts() {
        assert_eq!(evaluate_room_health(&[]).verdict, "empty");
        let mut dead = link(1, 20.0);
        dead.online = false;
        assert_eq!(evaluate_room_health(&[dead]).verdict, "offline");
    }

    #[test]
    fn absent_mesh_sync_is_informational_not_a_failure() {
        let mut a = link(1, 20.0);
        let mut b = link(2, 20.0);
        a.mesh_sync_valid = None;
        a.mesh_sync_staleness_ms = None;
        b.mesh_sync_valid = None;
        b.mesh_sync_staleness_ms = None;
        let health = evaluate_room_health(&[a, b]);
        assert_eq!(health.verdict, "aligned");
        assert!(health.issues.iter().any(|i| i.code == "mesh_sync_absent"));
    }
}
