//! Conservative overlap of cell faces on a shared line.

#[derive(Debug)]
pub(crate) struct FaceOverlap {
    pub left: usize,
    pub right: usize,
    pub length_m: f64,
}

/// Intersect two ordered partitions with common endpoints. Each intersection
/// identifies the exact measure to use for both sides of an exchange.
pub(crate) fn overlap(left: &[f64], right: &[f64]) -> Vec<FaceOverlap> {
    let mut faces = Vec::new();
    let (mut a, mut b) = (0, 0);
    while a + 1 < left.len() && b + 1 < right.len() {
        let end_a = left[a + 1];
        let end_b = right[b + 1];
        let end = if end_a <= end_b { end_a } else { end_b };
        let start = if left[a] >= right[b] {
            left[a]
        } else {
            right[b]
        };
        let length = end - start;
        if length > 0.0 {
            faces.push(FaceOverlap {
                left: a,
                right: b,
                length_m: length,
            });
        }
        if end_a <= end_b {
            a += 1;
        }
        if end_b <= end_a {
            b += 1;
        }
    }
    faces
}
