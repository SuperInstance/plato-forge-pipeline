use plato_forge_pipeline::{forge_pipeline, SeedInput};

fn main() {
    let seeds: Vec<SeedInput> = (0..59)
        .map(|i| SeedInput::new(
            format!("seed tile {} — extracted from plato-torch observation corpus", i),
            format!("doc-{}", i),
        ))
        .collect();

    println!("Running plato-forge-pipeline with {} seeds...", seeds.len());
    let result = forge_pipeline(seeds);

    println!("Tiles produced:  {}", result.tiles_produced.len());
    println!("Tiles rejected:  {}", result.tiles_rejected.len());
    println!("Compression:     {:.1}:1", result.compression_ratio);
    println!("Tier distribution:");
    let mut tiers: Vec<_> = result.tier_distribution.iter().collect();
    tiers.sort_by_key(|(k, _)| k.as_str());
    for (tier, count) in tiers {
        println!("  {}: {}", tier, count);
    }
    println!("Stage timings (ns):");
    for (stage, ns) in &result.stage_timings_ns {
        println!("  {}: {}ns", stage, ns);
    }
}
