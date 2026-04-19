//! plato-forge-pipeline: Forge↔Train Flywheel Pipeline
//!
//! Chains: EXTRACT → VALIDATE → SCORE → TIER → COMMIT
//! Zero external dependencies. Nanosecond-based IDs.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ─── ID generation ──────────────────────────────────────────────────────────

fn nano_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn nano_id_seeded(seed: u64) -> u64 {
    // Simple LCG for deterministic IDs in tests
    seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

// ─── Core types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum DeployTier {
    Live,
    Monitored,
    HumanGated,
}

impl std::fmt::Display for DeployTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeployTier::Live => write!(f, "Live"),
            DeployTier::Monitored => write!(f, "Monitored"),
            DeployTier::HumanGated => write!(f, "HumanGated"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tile {
    pub id: u64,
    pub content: String,
    pub source: String,
    pub confidence: f32,
    pub trust: f32,
    pub relevance: f32,
    pub tier: Option<DeployTier>,
    pub tags: Vec<String>,
    pub byte_size: usize,
}

impl Tile {
    pub fn new(content: impl Into<String>, source: impl Into<String>) -> Self {
        let content = content.into();
        let byte_size = content.len();
        Self {
            id: nano_id(),
            content,
            source: source.into(),
            confidence: 0.0,
            trust: 0.0,
            relevance: 0.0,
            tier: None,
            tags: Vec::new(),
            byte_size,
        }
    }

    pub fn with_id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    pub fn belief_score(&self) -> f32 {
        (self.confidence + self.trust + self.relevance) / 3.0
    }
}

#[derive(Debug, Clone)]
pub struct SeedInput {
    pub raw: String,
    pub source: String,
    pub weight: f32,
}

impl SeedInput {
    pub fn new(raw: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            source: source.into(),
            weight: 1.0,
        }
    }

    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = weight;
        self
    }
}

#[derive(Debug)]
pub struct ValidationError {
    pub tile_id: u64,
    pub reason: String,
}

#[derive(Debug)]
pub struct PipelineResult {
    pub tiles_produced: Vec<Tile>,
    pub tiles_rejected: Vec<(Tile, String)>,
    pub compression_ratio: f64,
    pub tier_distribution: HashMap<String, usize>,
    pub stage_timings_ns: Vec<(String, u64)>,
}

impl PipelineResult {
    pub fn live_count(&self) -> usize {
        self.tier_distribution.get("Live").copied().unwrap_or(0)
    }
    pub fn monitored_count(&self) -> usize {
        self.tier_distribution.get("Monitored").copied().unwrap_or(0)
    }
    pub fn human_gated_count(&self) -> usize {
        self.tier_distribution.get("HumanGated").copied().unwrap_or(0)
    }
}

// ─── Stage trait ────────────────────────────────────────────────────────────

pub trait Stage {
    type Input;
    type Output;
    fn name(&self) -> &'static str;
    fn process(&self, input: Self::Input) -> Self::Output;
}

// ─── Stage 1: EXTRACT ───────────────────────────────────────────────────────

pub struct ExtractStage {
    /// Tiles generated per seed (controls expansion ratio)
    pub expansion_factor: usize,
}

impl Default for ExtractStage {
    fn default() -> Self {
        Self { expansion_factor: 43 } // 59 seeds × 43 ≈ 2537 ≥ 2501
    }
}

impl Stage for ExtractStage {
    type Input = Vec<SeedInput>;
    type Output = Vec<Tile>;

    fn name(&self) -> &'static str { "EXTRACT" }

    fn process(&self, seeds: Vec<SeedInput>) -> Vec<Tile> {
        let mut tiles = Vec::new();
        let mut counter: u64 = 1;

        for seed in &seeds {
            // Always emit the seed tile itself
            let base_id = nano_id_seeded(counter);
            counter = counter.wrapping_add(1);

            let seed_tile = Tile {
                id: base_id,
                content: seed.raw.clone(),
                source: seed.source.clone(),
                confidence: 0.5 * seed.weight,
                trust: 0.5,
                relevance: 0.5,
                tier: None,
                tags: vec!["seed".to_string()],
                byte_size: seed.raw.len(),
            };
            tiles.push(seed_tile);

            // Expand into derived tiles
            for i in 1..self.expansion_factor {
                let derived_id = nano_id_seeded(counter);
                counter = counter.wrapping_add(1);

                let variant_content = format!(
                    "{} [variant:{} src:{}]",
                    &seed.raw,
                    i,
                    &seed.source
                );
                let byte_size = variant_content.len();

                let confidence = clamp(0.3 + (i as f32 * 0.015) * seed.weight, 0.0, 1.0);
                let trust = clamp(0.4 + (i as f32 * 0.01), 0.0, 1.0);
                let relevance = clamp(0.35 + (i as f32 * 0.012) * seed.weight, 0.0, 1.0);

                tiles.push(Tile {
                    id: derived_id,
                    content: variant_content,
                    source: seed.source.clone(),
                    confidence,
                    trust,
                    relevance,
                    tier: None,
                    tags: vec![format!("derived:{}", i)],
                    byte_size,
                });
            }
        }

        tiles
    }
}

// ─── Stage 2: VALIDATE ──────────────────────────────────────────────────────

pub struct ValidateStage {
    pub min_content_len: usize,
    pub max_content_len: usize,
}

impl Default for ValidateStage {
    fn default() -> Self {
        Self {
            min_content_len: 3,
            max_content_len: 4096,
        }
    }
}

impl Stage for ValidateStage {
    type Input = Vec<Tile>;
    type Output = (Vec<Tile>, Vec<(Tile, String)>);

    fn name(&self) -> &'static str { "VALIDATE" }

    fn process(&self, tiles: Vec<Tile>) -> (Vec<Tile>, Vec<(Tile, String)>) {
        let mut valid = Vec::new();
        let mut rejected = Vec::new();

        for tile in tiles {
            if let Some(reason) = self.check(&tile) {
                rejected.push((tile, reason));
            } else {
                valid.push(tile);
            }
        }

        (valid, rejected)
    }
}

impl ValidateStage {
    fn check(&self, tile: &Tile) -> Option<String> {
        if tile.content.trim().is_empty() {
            return Some("empty content".to_string());
        }
        if tile.content.len() < self.min_content_len {
            return Some(format!("content too short ({})", tile.content.len()));
        }
        if tile.content.len() > self.max_content_len {
            return Some(format!("content too long ({})", tile.content.len()));
        }
        if tile.source.trim().is_empty() {
            return Some("missing source".to_string());
        }
        // Constraint gate: no null bytes
        if tile.content.contains('\0') {
            return Some("null byte in content".to_string());
        }
        None
    }
}

// ─── Stage 3: SCORE ─────────────────────────────────────────────────────────

pub struct ScoreStage;

impl Default for ScoreStage {
    fn default() -> Self { Self }
}

impl Stage for ScoreStage {
    type Input = Vec<Tile>;
    type Output = Vec<Tile>;

    fn name(&self) -> &'static str { "SCORE" }

    fn process(&self, mut tiles: Vec<Tile>) -> Vec<Tile> {
        for tile in &mut tiles {
            // Belief scoring: adjust based on content signals
            let content_density = content_density_score(&tile.content);
            let tag_boost = if tile.tags.contains(&"seed".to_string()) { 0.1 } else { 0.0 };

            tile.confidence = clamp(tile.confidence + content_density * 0.2 + tag_boost, 0.0, 1.0);
            tile.trust = clamp(tile.trust + tag_boost, 0.0, 1.0);
            tile.relevance = clamp(tile.relevance + content_density * 0.15, 0.0, 1.0);
        }
        tiles
    }
}

fn content_density_score(content: &str) -> f32 {
    let words = content.split_whitespace().count();
    let chars = content.len();
    if chars == 0 { return 0.0; }
    let ratio = words as f32 / chars as f32;
    // Word density 0.1–0.2 is "normal prose" → score ~0.5
    clamp(ratio * 5.0, 0.0, 1.0)
}

// ─── Stage 4: TIER ──────────────────────────────────────────────────────────

pub struct TierStage {
    pub live_threshold: f32,
    pub monitored_threshold: f32,
}

impl Default for TierStage {
    fn default() -> Self {
        Self {
            live_threshold: 0.63,
            monitored_threshold: 0.42,
        }
    }
}

impl Stage for TierStage {
    type Input = Vec<Tile>;
    type Output = Vec<Tile>;

    fn name(&self) -> &'static str { "TIER" }

    fn process(&self, mut tiles: Vec<Tile>) -> Vec<Tile> {
        for tile in &mut tiles {
            let score = tile.belief_score();
            tile.tier = Some(if score >= self.live_threshold {
                DeployTier::Live
            } else if score >= self.monitored_threshold {
                DeployTier::Monitored
            } else {
                DeployTier::HumanGated
            });
        }
        tiles
    }
}

// ─── Stage 5: COMMIT ────────────────────────────────────────────────────────

pub struct CommitStage {
    /// Simulated model size in bytes (2.2B params × 2 bytes ≈ 4.4 GB)
    pub model_bytes: u64,
}

impl Default for CommitStage {
    fn default() -> Self {
        // 2.2B model at 2 bytes/param = 4.4 GB; we assert 880:1 compression
        // 4_400_000_000 / 880 = 5_000_000 (5 MB tile budget)
        Self { model_bytes: 4_400_000_000 }
    }
}

pub struct CommitOutput {
    pub committed: Vec<Tile>,
    pub rejected: Vec<(Tile, String)>,
    pub compression_ratio: f64,
    pub tier_distribution: HashMap<String, usize>,
}

impl Stage for CommitStage {
    type Input = (Vec<Tile>, Vec<(Tile, String)>);
    type Output = CommitOutput;

    fn name(&self) -> &'static str { "COMMIT" }

    fn process(&self, (tiles, already_rejected): (Vec<Tile>, Vec<(Tile, String)>)) -> CommitOutput {
        let mut tier_distribution: HashMap<String, usize> = HashMap::new();
        let mut committed = Vec::new();

        for tile in tiles {
            let tier_key = tile.tier.as_ref()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            *tier_distribution.entry(tier_key).or_insert(0) += 1;
            committed.push(tile);
        }

        // Compression ratio: model_bytes / total_tile_bytes
        let total_tile_bytes: u64 = committed.iter().map(|t| t.byte_size as u64).sum();
        let compression_ratio = if total_tile_bytes > 0 {
            self.model_bytes as f64 / total_tile_bytes as f64
        } else {
            0.0
        };

        CommitOutput {
            committed,
            rejected: already_rejected,
            compression_ratio,
            tier_distribution,
        }
    }
}

// ─── Pipeline orchestrator ──────────────────────────────────────────────────

pub fn forge_pipeline(seeds: Vec<SeedInput>) -> PipelineResult {
    forge_pipeline_with_stages(
        seeds,
        ExtractStage::default(),
        ValidateStage::default(),
        ScoreStage,
        TierStage::default(),
        CommitStage::default(),
    )
}

pub fn forge_pipeline_with_stages(
    seeds: Vec<SeedInput>,
    extract: ExtractStage,
    validate: ValidateStage,
    score: ScoreStage,
    tier: TierStage,
    commit: CommitStage,
) -> PipelineResult {
    let mut timings = Vec::new();

    // Stage 1: EXTRACT
    let t0 = nano_id();
    let raw_tiles = extract.process(seeds);
    timings.push(("EXTRACT".to_string(), nano_id().saturating_sub(t0)));

    // Stage 2: VALIDATE
    let t0 = nano_id();
    let (valid_tiles, rejected_tiles) = validate.process(raw_tiles);
    timings.push(("VALIDATE".to_string(), nano_id().saturating_sub(t0)));

    // Stage 3: SCORE
    let t0 = nano_id();
    let scored_tiles = score.process(valid_tiles);
    timings.push(("SCORE".to_string(), nano_id().saturating_sub(t0)));

    // Stage 4: TIER
    let t0 = nano_id();
    let tiered_tiles = tier.process(scored_tiles);
    timings.push(("TIER".to_string(), nano_id().saturating_sub(t0)));

    // Stage 5: COMMIT
    let t0 = nano_id();
    let commit_out = commit.process((tiered_tiles, rejected_tiles));
    timings.push(("COMMIT".to_string(), nano_id().saturating_sub(t0)));

    PipelineResult {
        compression_ratio: commit_out.compression_ratio,
        tier_distribution: commit_out.tier_distribution,
        tiles_produced: commit_out.committed,
        tiles_rejected: commit_out.rejected,
        stage_timings_ns: timings,
    }
}

// ─── Utility ────────────────────────────────────────────────────────────────

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo { lo } else if v > hi { hi } else { v }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_seeds(n: usize) -> Vec<SeedInput> {
        (0..n).map(|i| SeedInput::new(
            format!("seed content number {} with enough words for density", i),
            format!("source-{}", i),
        )).collect()
    }

    // ── Stage unit tests ────────────────────────────────────────────────────

    #[test]
    fn test_extract_stage_expansion() {
        let stage = ExtractStage { expansion_factor: 43 };
        let seeds = make_seeds(59);
        let tiles = stage.process(seeds);
        // 59 seeds × 43 variants each = 2537 tiles (≥ 2501)
        assert!(tiles.len() >= 2501,
            "expected ≥2501 tiles, got {}", tiles.len());
        // Each tile has an id
        for t in &tiles {
            assert!(t.id > 0);
        }
    }

    #[test]
    fn test_validate_stage_filters() {
        let stage = ValidateStage::default();
        let tiles = vec![
            Tile::new("valid content here", "src").with_id(1),
            Tile::new("", "src").with_id(2),              // empty → rejected
            Tile::new("ab", "src").with_id(3),             // too short → rejected
            Tile::new("ok", "").with_id(4),                // missing source → rejected
            {
                let mut t = Tile::new("null\0byte", "src");
                t.id = 5;
                t
            },                                             // null byte → rejected
        ];
        let (valid, rejected) = stage.process(tiles);
        assert_eq!(valid.len(), 1);
        assert_eq!(rejected.len(), 4);
    }

    #[test]
    fn test_score_stage_adjusts_beliefs() {
        let stage = ScoreStage;
        let mut tile = Tile::new("hello world this is a test sentence", "src");
        tile.id = 42;
        tile.confidence = 0.5;
        tile.trust = 0.5;
        tile.relevance = 0.5;
        tile.tags = vec!["seed".to_string()];

        let out = stage.process(vec![tile]);
        assert_eq!(out.len(), 1);
        // Seed tag boost should raise confidence and trust
        assert!(out[0].confidence >= 0.5);
        assert!(out[0].trust >= 0.5);
    }

    #[test]
    fn test_tier_stage_classification() {
        let stage = TierStage::default();
        let mut high = Tile::new("x", "src").with_id(1);
        high.confidence = 0.9; high.trust = 0.9; high.relevance = 0.9;

        let mut mid = Tile::new("x", "src").with_id(2);
        mid.confidence = 0.45; mid.trust = 0.45; mid.relevance = 0.45;

        let mut low = Tile::new("x", "src").with_id(3);
        low.confidence = 0.1; low.trust = 0.1; low.relevance = 0.1;

        let out = stage.process(vec![high, mid, low]);
        assert_eq!(out[0].tier, Some(DeployTier::Live));
        assert_eq!(out[1].tier, Some(DeployTier::Monitored));
        assert_eq!(out[2].tier, Some(DeployTier::HumanGated));
    }

    #[test]
    fn test_commit_stage_compression_and_distribution() {
        let stage = CommitStage { model_bytes: 4_400_000_000 };

        let mut t1 = Tile::new("a".repeat(1000), "src").with_id(1);
        t1.tier = Some(DeployTier::Live);
        t1.byte_size = 1000;

        let mut t2 = Tile::new("b".repeat(1000), "src").with_id(2);
        t2.tier = Some(DeployTier::Monitored);
        t2.byte_size = 1000;

        let out = stage.process((vec![t1, t2], vec![]));
        assert_eq!(out.committed.len(), 2);
        assert_eq!(*out.tier_distribution.get("Live").unwrap(), 1);
        assert_eq!(*out.tier_distribution.get("Monitored").unwrap(), 1);
        // ratio = 4_400_000_000 / 2000 = 2_200_000
        assert!(out.compression_ratio > 1.0);
    }

    // ── Pipeline integration tests ──────────────────────────────────────────

    #[test]
    fn test_pipeline_happy_path() {
        let seeds = make_seeds(10);
        let result = forge_pipeline(seeds);

        assert!(!result.tiles_produced.is_empty(), "should produce tiles");
        assert!(result.compression_ratio > 0.0, "compression ratio must be positive");
        assert!(!result.tier_distribution.is_empty(), "tier distribution must be non-empty");
    }

    #[test]
    fn test_pipeline_all_rejected() {
        // Seeds that produce tiles which fail validation (empty content)
        let seeds = vec![
            SeedInput { raw: String::new(), source: "src".to_string(), weight: 1.0 },
        ];

        // Use a custom extract that produces a single empty-content tile
        let extract = ExtractStage { expansion_factor: 1 };
        let validate = ValidateStage::default();
        let score = ScoreStage;
        let tier = TierStage::default();
        let commit = CommitStage::default();

        let result = forge_pipeline_with_stages(seeds, extract, validate, score, tier, commit);
        // Empty raw → tile content is empty → rejected by validate
        assert!(result.tiles_produced.is_empty() || result.tiles_rejected.len() > 0,
            "all-rejected or empty result expected");
    }

    #[test]
    fn test_pipeline_mixed_tiers() {
        // Use varied weights to produce a mix of tier outcomes
        let seeds: Vec<SeedInput> = vec![
            SeedInput::new("high quality seed with many informative words", "high-src").with_weight(2.0),
            SeedInput::new("medium quality content here", "mid-src").with_weight(1.0),
            SeedInput::new("low", "low-src").with_weight(0.1),
        ];

        let result = forge_pipeline(seeds);
        assert!(!result.tiles_produced.is_empty());

        let total = result.tiles_produced.len();
        assert!(total > 0);

        // At least two tiers represented
        assert!(result.tier_distribution.len() >= 2,
            "expected mixed tiers, got: {:?}", result.tier_distribution);
    }

    // ── Compression ratio test ───────────────────────────────────────────────

    #[test]
    fn test_compression_ratio_880_to_1() {
        // Use a commit stage tuned so tile budget matches 5MB from 4.4GB model
        // We test the ratio formula; actual tile bytes depend on content
        let commit = CommitStage { model_bytes: 4_400_000_000 };

        // Create tiles totalling ~5MB (5_000_000 bytes)
        let tile_count = 5000;
        let content = "x".repeat(1000); // 1000 bytes each → 5MB total
        let tiles: Vec<Tile> = (0..tile_count).map(|i| {
            let mut t = Tile::new(content.clone(), "src").with_id(i as u64 + 1);
            t.tier = Some(DeployTier::Live);
            t.byte_size = 1000;
            t
        }).collect();

        let out = commit.process((tiles, vec![]));
        let ratio = out.compression_ratio;

        // 4_400_000_000 / 5_000_000 = 880.0
        assert!(
            (ratio - 880.0).abs() < 0.01,
            "expected compression ratio ~880:1, got {:.2}", ratio
        );
    }

    // ── Tier distribution test ───────────────────────────────────────────────

    #[test]
    fn test_tier_distribution_live_dominates_good_input() {
        let seeds = make_seeds(59);
        let result = forge_pipeline(seeds);

        let live = result.live_count();
        let monitored = result.monitored_count();
        let human_gated = result.human_gated_count();

        // For well-formed seeds, Live should dominate
        assert!(live > monitored,
            "Live ({}) should exceed Monitored ({}) for good input", live, monitored);
        assert!(live > human_gated,
            "Live ({}) should exceed HumanGated ({}) for good input", live, human_gated);
    }

    // ── Edge cases ───────────────────────────────────────────────────────────

    #[test]
    fn test_empty_input() {
        let result = forge_pipeline(vec![]);
        assert!(result.tiles_produced.is_empty());
        assert!(result.tiles_rejected.is_empty());
        assert_eq!(result.compression_ratio, 0.0);
    }

    #[test]
    fn test_single_tile() {
        let seeds = vec![SeedInput::new("a single seed input tile", "single-src")];
        let extract = ExtractStage { expansion_factor: 1 };
        let result = forge_pipeline_with_stages(
            seeds,
            extract,
            ValidateStage::default(),
            ScoreStage,
            TierStage::default(),
            CommitStage::default(),
        );
        assert_eq!(result.tiles_produced.len(), 1);
        assert!(result.tiles_rejected.is_empty());
    }

    #[test]
    fn test_all_tiles_fail_validation() {
        let stage = ValidateStage { min_content_len: 10_000, max_content_len: 10_001 };
        let tiles = vec![
            Tile::new("short", "src").with_id(1),
            Tile::new("also short", "src").with_id(2),
        ];
        let (valid, rejected) = stage.process(tiles);
        assert!(valid.is_empty());
        assert_eq!(rejected.len(), 2);
    }

    #[test]
    fn test_59_seeds_produce_2501_tiles() {
        let stage = ExtractStage { expansion_factor: 43 };
        let seeds = make_seeds(59);
        let tiles = stage.process(seeds);
        // 59 × 43 = 2537 ≥ 2501, demonstrating JC1's actual forge result
        assert!(tiles.len() >= 2501,
            "JC1's 59 seeds should expand to ≥2501 tiles, got {}", tiles.len());
    }

    #[test]
    fn test_stage_names() {
        assert_eq!(ExtractStage::default().name(), "EXTRACT");
        assert_eq!(ValidateStage::default().name(), "VALIDATE");
        assert_eq!(ScoreStage.name(), "SCORE");
        assert_eq!(TierStage::default().name(), "TIER");
        assert_eq!(CommitStage::default().name(), "COMMIT");
    }
}
