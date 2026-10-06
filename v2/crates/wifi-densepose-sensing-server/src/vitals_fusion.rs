//! Room-level fusion of per-node vital-sign readings.
//!
//! Previously the published vitals were simply those of whichever node's UDP
//! packet arrived last, so with several ESP32 nodes the displayed rate flipped
//! between nodes and a single bad link could overwrite two good ones. This
//! module combines the nodes of one room into one reading:
//!
//! * nodes that are in motion are ignored (body movement swamps the
//!   respiration/cardiac band),
//! * each band is fused independently by finding the cluster of nodes that
//!   agree within a tolerance and weighting them by confidence and quality,
//! * agreement between independent nodes raises confidence; a confident node
//!   that disagrees with the winning cluster lowers it.
//!
//! It is pure and deterministic. It does not authorize publication — the
//! calibration, single-occupant, confidence and quality gates still apply to
//! the fused result.

use serde::Serialize;

/// Maximum spread, in breaths per minute, for two nodes to "agree".
pub const BREATHING_AGREEMENT_BPM: f64 = 2.0;
/// Maximum spread, in beats per minute, for two nodes to "agree".
pub const HEART_AGREEMENT_BPM: f64 = 6.0;
const AGREEMENT_BONUS_PER_NODE: f64 = 0.08;
const AGREEMENT_BONUS_CAP: f64 = 0.16;
const DISSENT_CONFIDENCE: f64 = 0.55;
const DISSENT_FACTOR: f64 = 0.7;

#[derive(Debug, Clone, Default)]
pub struct NodeVitalReading {
    pub node_id: u8,
    pub breathing_bpm: Option<f64>,
    pub heart_bpm: Option<f64>,
    pub breathing_confidence: f64,
    pub heart_confidence: f64,
    pub signal_quality: f64,
    /// The node currently classifies the room as "active" (walking etc.).
    pub moving: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FusedBand {
    pub value: Option<f64>,
    pub confidence: f64,
    pub contributing: Vec<u8>,
    pub dissenting: Vec<u8>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FusedVitals {
    pub breathing: FusedBand,
    pub heart: FusedBand,
    pub signal_quality: f64,
    pub nodes_considered: usize,
    pub nodes_moving: usize,
}

struct Candidate {
    node_id: u8,
    value: f64,
    confidence: f64,
    quality: f64,
}

fn fuse_band(candidates: &[Candidate], tolerance: f64) -> (FusedBand, Vec<f64>) {
    if candidates.is_empty() {
        return (FusedBand::default(), Vec::new());
    }
    let weight = |c: &Candidate| c.confidence * (0.25 + 0.75 * c.quality.clamp(0.0, 1.0));
    // Winning cluster: the candidate whose tolerance neighbourhood carries the
    // most weight (ties broken by the higher individual confidence).
    let mut best: Option<(f64, f64, usize)> = None; // (support, confidence, index)
    for (i, centre) in candidates.iter().enumerate() {
        let support: f64 = candidates
            .iter()
            .filter(|c| (c.value - centre.value).abs() <= tolerance)
            .map(weight)
            .sum();
        let better = match best {
            None => true,
            Some((s, conf, _)) => {
                support > s + 1e-12 || ((support - s).abs() <= 1e-12 && centre.confidence > conf)
            }
        };
        if better {
            best = Some((support, centre.confidence, i));
        }
    }
    let centre = &candidates[best.map(|b| b.2).unwrap_or(0)];
    let cluster: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| (c.value - centre.value).abs() <= tolerance)
        .collect();
    let dissenters: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| (c.value - centre.value).abs() > tolerance)
        .collect();
    let weight_sum: f64 = cluster.iter().map(|c| weight(c)).sum();
    if weight_sum <= 0.0 {
        return (FusedBand::default(), Vec::new());
    }
    let value = cluster.iter().map(|c| c.value * weight(c)).sum::<f64>() / weight_sum;
    let mean_conf = cluster.iter().map(|c| c.confidence * weight(c)).sum::<f64>() / weight_sum;
    let bonus = (AGREEMENT_BONUS_PER_NODE * (cluster.len() as f64 - 1.0)).min(AGREEMENT_BONUS_CAP);
    let mut confidence = mean_conf + bonus;
    if dissenters.iter().any(|c| c.confidence >= DISSENT_CONFIDENCE) {
        confidence *= DISSENT_FACTOR;
    }
    let qualities = cluster.iter().map(|c| c.quality).collect();
    (
        FusedBand {
            value: Some(value),
            confidence: confidence.clamp(0.0, 1.0),
            contributing: cluster.iter().map(|c| c.node_id).collect(),
            dissenting: dissenters.iter().map(|c| c.node_id).collect(),
        },
        qualities,
    )
}

/// Fuse the readings of the nodes that belong to one room.
pub fn fuse(readings: &[NodeVitalReading]) -> FusedVitals {
    let moving = readings.iter().filter(|r| r.moving).count();
    let still: Vec<&NodeVitalReading> = readings.iter().filter(|r| !r.moving).collect();

    let br: Vec<Candidate> = still
        .iter()
        .filter_map(|r| {
            r.breathing_bpm
                .filter(|v| v.is_finite() && *v > 0.0)
                .map(|value| Candidate {
                    node_id: r.node_id,
                    value,
                    confidence: r.breathing_confidence.clamp(0.0, 1.0),
                    quality: r.signal_quality,
                })
        })
        .collect();
    let hr: Vec<Candidate> = still
        .iter()
        .filter_map(|r| {
            r.heart_bpm
                .filter(|v| v.is_finite() && *v > 0.0)
                .map(|value| Candidate {
                    node_id: r.node_id,
                    value,
                    confidence: r.heart_confidence.clamp(0.0, 1.0),
                    quality: r.signal_quality,
                })
        })
        .collect();

    let (breathing, mut qualities) = fuse_band(&br, BREATHING_AGREEMENT_BPM);
    let (heart, heart_qualities) = fuse_band(&hr, HEART_AGREEMENT_BPM);
    qualities.extend(heart_qualities);
    let signal_quality = if qualities.is_empty() {
        // No band survived: report the best quality among still nodes so the
        // caller can still tell "noisy" from "nothing there".
        still
            .iter()
            .map(|r| r.signal_quality)
            .fold(0.0, f64::max)
            .clamp(0.0, 1.0)
    } else {
        (qualities.iter().sum::<f64>() / qualities.len() as f64).clamp(0.0, 1.0)
    };

    FusedVitals {
        breathing,
        heart,
        signal_quality,
        nodes_considered: readings.len(),
        nodes_moving: moving,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: u8, br: Option<f64>, brc: f64, hr: Option<f64>, hrc: f64, q: f64) -> NodeVitalReading {
        NodeVitalReading {
            node_id: id,
            breathing_bpm: br,
            heart_bpm: hr,
            breathing_confidence: brc,
            heart_confidence: hrc,
            signal_quality: q,
            moving: false,
        }
    }

    #[test]
    fn agreeing_nodes_raise_confidence_over_any_single_node() {
        let one = fuse(&[r(1, Some(15.0), 0.6, None, 0.0, 0.7)]);
        let three = fuse(&[
            r(1, Some(15.0), 0.6, None, 0.0, 0.7),
            r(2, Some(15.4), 0.6, None, 0.0, 0.7),
            r(3, Some(14.8), 0.6, None, 0.0, 0.7),
        ]);
        assert!(three.breathing.confidence > one.breathing.confidence);
        assert_eq!(three.breathing.contributing.len(), 3);
        assert!((three.breathing.value.unwrap() - 15.07).abs() < 0.2);
    }

    #[test]
    fn a_confident_outlier_does_not_win_and_reduces_confidence() {
        let fused = fuse(&[
            r(1, Some(15.0), 0.8, None, 0.0, 0.8),
            r(2, Some(15.2), 0.8, None, 0.0, 0.8),
            r(3, Some(27.0), 0.7, None, 0.0, 0.8),
        ]);
        assert!((fused.breathing.value.unwrap() - 15.1).abs() < 0.2);
        assert_eq!(fused.breathing.dissenting, vec![3]);
        let consensus_only = fuse(&[
            r(1, Some(15.0), 0.8, None, 0.0, 0.8),
            r(2, Some(15.2), 0.8, None, 0.0, 0.8),
        ]);
        assert!(fused.breathing.confidence < consensus_only.breathing.confidence);
    }

    #[test]
    fn weak_nodes_cannot_outvote_one_strong_node() {
        let fused = fuse(&[
            r(1, Some(16.0), 0.9, None, 0.0, 0.9),
            r(2, Some(24.0), 0.26, None, 0.0, 0.3),
            r(3, Some(8.0), 0.26, None, 0.0, 0.3),
        ]);
        assert!((fused.breathing.value.unwrap() - 16.0).abs() < 0.1);
    }

    #[test]
    fn moving_nodes_are_ignored() {
        let mut moving = r(2, Some(30.0), 0.9, Some(110.0), 0.9, 0.9);
        moving.moving = true;
        let fused = fuse(&[r(1, Some(14.0), 0.7, Some(70.0), 0.6, 0.8), moving]);
        assert_eq!(fused.nodes_moving, 1);
        assert!((fused.breathing.value.unwrap() - 14.0).abs() < 1e-9);
        assert!((fused.heart.value.unwrap() - 70.0).abs() < 1e-9);
    }

    #[test]
    fn nothing_to_fuse_yields_empty_bands() {
        let fused = fuse(&[r(1, None, 0.0, None, 0.0, 0.2), r(2, None, 0.0, None, 0.0, 0.5)]);
        assert!(fused.breathing.value.is_none() && fused.heart.value.is_none());
        assert_eq!(fused.breathing.confidence, 0.0);
        assert!((fused.signal_quality - 0.5).abs() < 1e-9);
        assert!(fuse(&[]).breathing.value.is_none());
    }
}
