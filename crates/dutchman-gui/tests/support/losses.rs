// Synthetic loss notes; no native design is needed to exercise result-card layout.
pub fn report() -> serde_json::Value {
    serde_json::json!({"ok": true, "mode": "import", "losses": [
        {"level": "Degraded", "subject": "feature f3d:model:feature#69:f3d%3AFusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat6:Fillet3:1780", "message": "edge selection cannot resolve historical midpoints"},
        {"level": "Dropped", "subject": "feature", "message": format!("{} cannot be mapped yet", "UnbrokenFeatureId".repeat(20))},
        {"level": "Info", "subject": "history", "message": "The original geometry is kept alongside the resolved sketches."}
    ]})
}
