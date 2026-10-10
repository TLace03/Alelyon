//! How many relevant papers has nothing found yet? A capture-recapture estimate over two ways of finding papers.
//!
//! If keyword search finds n1 relevant papers, following citations finds n2, and m papers were found by both, then
//! the overlap says how thorough each method is: a method that finds most of what the other found is probably
//! finding most of what exists. Chapman's form of the Lincoln-Petersen estimator turns that into a total.
//!
//! The estimate assumes the two methods find papers independently. They do not quite: a paper that is easy to find
//! by keyword (well titled, much cited) is easier to find by citation too. That positive dependence makes the
//! estimate of what is missing too SMALL, so it is a floor on the gap, not a guarantee of its absence.

/// The estimate, or the reason there is none.
#[derive(Debug, Clone, PartialEq)]
pub enum Coverage {
    Estimated {
        /// Distinct relevant papers actually held (n1 + n2 - m).
        found: u64,
        /// Chapman's estimate of all relevant papers, found or not.
        estimated_total: f64,
        /// estimated_total - found, never below zero.
        estimated_missing: f64,
        /// A normal-approximation 95% interval on the missing count, clipped at zero.
        missing_95: (f64, f64),
    },
    /// No estimate: the methods' overlap carries no information about the remainder.
    Unmeasured(&'static str),
}

impl Coverage {
    /// The estimate in words, for a person or a saved record.
    pub fn summary(&self) -> String {
        match self {
            Coverage::Estimated { estimated_missing, missing_95, .. } => format!(
                "about {:.0} relevant papers not found (95%: {:.0} to {:.0}; a floor, the methods are not independent)",
                estimated_missing, missing_95.0, missing_95.1
            ),
            Coverage::Unmeasured(why) => format!("UNMEASURED: {why}"),
        }
    }
}

pub fn chapman(n1: u64, n2: u64, m: u64) -> Coverage {
    if m > n1.min(n2) {
        return Coverage::Unmeasured("the overlap is larger than one of the methods' finds (inconsistent counts)");
    }
    if n1 == 0 || n2 == 0 {
        return Coverage::Unmeasured("one of the two methods found nothing, so their overlap says nothing");
    }
    if m == 0 {
        return Coverage::Unmeasured("the two methods found no paper in common, so the remainder cannot be bounded");
    }
    let (a, b, m) = (n1 as f64, n2 as f64, m as f64);
    let found = (n1 + n2) as f64 - m;
    let total = (a + 1.0) * (b + 1.0) / (m + 1.0) - 1.0;
    let var = (a + 1.0) * (b + 1.0) * (a - m) * (b - m) / ((m + 1.0).powi(2) * (m + 2.0));
    let missing = (total - found).max(0.0);
    let half = 1.96 * var.sqrt();
    Coverage::Estimated {
        found: found as u64,
        estimated_total: total,
        estimated_missing: missing,
        missing_95: ((missing - half).max(0.0), (missing + half).max(0.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_overlap_estimates_nothing_missing() {
        let Coverage::Estimated { found, estimated_missing, .. } = chapman(50, 50, 50) else { panic!() };
        assert_eq!(found, 50);
        assert!(estimated_missing < 1e-9);
    }

    #[test]
    fn small_overlap_estimates_a_large_remainder() {
        // 100 and 100 with 10 in common: about 900 papers exist, 190 are held.
        let Coverage::Estimated { found, estimated_total, estimated_missing, missing_95 } = chapman(100, 100, 10) else { panic!() };
        assert_eq!(found, 190);
        // (101 * 101) / 11 - 1 = 926.36...
        assert!((estimated_total - 926.3636).abs() < 0.001, "{estimated_total}");
        assert!((estimated_missing - 736.3636).abs() < 0.001);
        assert!(missing_95.0 < estimated_missing && estimated_missing < missing_95.1);
    }

    #[test]
    fn no_overlap_is_unmeasured_not_zero() {
        assert!(matches!(chapman(10, 10, 0), Coverage::Unmeasured(_)));
        assert!(matches!(chapman(0, 10, 0), Coverage::Unmeasured(_)));
        assert!(matches!(chapman(3, 10, 5), Coverage::Unmeasured(_)));
    }
}
