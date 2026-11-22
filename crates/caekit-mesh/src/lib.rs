//! Mesh geometry, region assignments, and mesh quality checks.

mod line;
pub mod tetra;

pub use line::{AxisFrame, GeometryRevision, LineMesh, MeshMetrics, MeshQuality};
pub use tetra::{FaceRegion, TetraEntities, TetraMesh};

fn length(vector: [f64; 3]) -> f64 {
    let mut maximum = 0.0_f64;
    for value in vector {
        if value.is_nan() {
            return value;
        }
        maximum = maximum.max(value.abs());
    }
    if maximum == 0.0 || maximum.is_infinite() {
        return maximum;
    }
    // A power-of-two scale keeps the divisions exact for components large
    // enough to affect the sum. Use the smallest normal scale for subnormals.
    let scale = f64::from_bits((maximum.to_bits() & (0x7ff << 52)).max(1 << 52));
    let sum: f64 = vector
        .into_iter()
        .map(|value| {
            let scaled = value / scale;
            scaled * scaled
        })
        .sum();
    scale * sum.sqrt()
}
