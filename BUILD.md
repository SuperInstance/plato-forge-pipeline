# BUILD plato-forge-pipeline — Forge↔Train Flywheel

## What to Build
The capstone pipeline that chains: extract tiles → validate → train config → DCS assign → deploy tier → persist.

This is the flywheel Oracle1 mapped in tile-forge-plato-torch-convergence.md:
```
JC1 extracts tiles from docs → Oracle1 trains rooms from tiles → 
Ensign exported → JC1 loads ensign on Jetson → Better tile extraction → Better rooms → Better ensigns
```

## Pipeline Stages (each is a pure function)
1. EXTRACT: Take raw input (documents, patterns, observations) → produce tiles
2. VALIDATE: Check each tile against plato-tile-spec format + constraint gates
3. SCORE: Apply belief scoring (confidence, trust, relevance) to each tile
4. TIER: Classify tiles into deploy tiers (Live/Monitored/HumanGated)
5. COMMIT: Persist valid tiles, track rejected tiles for review

## Key Numbers
- 880:1 compression (2.2B model → 5MB tiles) — assert in test
- 59 seed tiles → 2,501 tiles (JC1's actual forge result) — demonstrate in test
- Each stage is independently testable
- The whole pipeline is a single fn: forge_pipeline(seeds) -> PipelineResult

## Design
- Zero external deps, cargo 1.75 compatible
- Each stage is a struct implementing a Stage trait
- PipelineResult contains: tiles_produced, tiles_rejected, compression_ratio, tier_distribution
- Use nanosecond-based IDs (no uuid)

## Test Requirements
- 5 individual stage tests
- 3 pipeline integration tests (happy path, all-rejected, mixed tiers)
- Compression ratio test (880:1 asserted)
- Tier distribution test (Live > Monitored > HumanGated for good input)
- Edge cases: empty input, single tile, all tiles fail validation

BUILD IT NOW. Write Cargo.toml, src/lib.rs with all 5 stages + pipeline, comprehensive tests.
No uuid crate. Use nanosecond-based IDs. Zero external dependencies.
Push to GitHub when tests pass.
