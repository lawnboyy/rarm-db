use core::f64;
use std::hash::Hash;
use std::{cmp::Ordering, hash::Hasher};

use serde::{Deserialize, Serialize};

/// New Type wrapper for f64 that implements the Eq and Ord traits so
/// we can use it in indexes. The f64 does not implement these traits
/// natively because of the NaN value which is not equal or comparable
/// to another NaN value. We wrap the f64 and implement the traits
/// to treat NaN == NanN as true and greater than anything other than
/// NaN.
#[derive(Debug, Deserialize, Clone, Copy, Serialize)]
pub struct OrderedFloat(pub f64);

impl PartialEq for OrderedFloat {
    fn eq(&self, other: &Self) -> bool {
        if self.0.is_nan() && other.0.is_nan() {
            true
        } else {
            self.0 == other.0
        }
    }
}

impl Eq for OrderedFloat {}

/// Implement the Hash trait for our OrderedFloat because the f64 type does not implement the
/// Hash trait because -0.0 and 0.0 are equivalent and NaN == NaN is false. So we have to
/// handle those cases to get a consistent hash value for an f64.
impl Hash for OrderedFloat {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        // Create a copy of the actual value which we can coerce into a value
        // that can be consistently hashed if it is -0.0 which won't hash the
        // same as 0.0. We also handle NaN which will not hash the same for
        // every instance.
        let mut coerced_value = self.0;
        if self.0 == -0.0 {
            // Force the value to be positive 0.0 and hash that...
            coerced_value = 0.0;
        } else if self.0.is_nan() {
            // Force it to be a canonical NaN hash value.
            coerced_value = f64::NAN;
        }

        let bits = coerced_value.to_bits();
        bits.hash(state);
    }
}

impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // This ensures operators <, >, == use your custom logic below
        Some(self.cmp(other))
    }
}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Do a standard partial comparison...
        match self.0.partial_cmp(&other.0) {
            // If we get an ordering, then we can just use it...
            Some(ordering) => ordering,
            // If we get None back, then one or both values are NaN
            None => {
                if self.0.is_nan() {
                    // Treat NaN == Naan as true
                    if other.0.is_nan() {
                        Ordering::Equal
                    } else {
                        // Treat NaN as greater than all other values.
                        Ordering::Greater
                    }
                } else {
                    // If self is a number and other is NaN, then return self < NaN
                    Ordering::Less
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cmp::Ordering, collections::HashSet, hash::DefaultHasher};

    #[test]
    fn test_equality_normal_numbers() {
        let a = OrderedFloat(1.5);
        let b = OrderedFloat(1.5);
        let c = OrderedFloat(2.0);

        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_equality_nan() {
        let nan1 = OrderedFloat(f64::NAN);
        let nan2 = OrderedFloat(f64::NAN);
        let num = OrderedFloat(1.0);

        // Standard f64 says NaN != NaN, but we want Total Equality
        assert_eq!(nan1, nan2, "NaN should equal NaN");
        assert_ne!(nan1, num, "NaN should not equal a number");
    }

    #[test]
    fn test_ordering_normal_numbers() {
        let a = OrderedFloat(1.0);
        let b = OrderedFloat(2.0);
        let c = OrderedFloat(1.0);

        assert!(a < b);
        assert!(b > a);
        assert!(a <= c);
        assert!(a >= c);
        assert_eq!(a.cmp(&b), Ordering::Less);
        assert_eq!(b.cmp(&a), Ordering::Greater);
    }

    #[test]
    fn test_ordering_nan() {
        let nan = OrderedFloat(f64::NAN);
        let inf = OrderedFloat(f64::INFINITY);
        let neg_inf = OrderedFloat(f64::NEG_INFINITY);
        let zero = OrderedFloat(0.0);

        // NaN should be greater than everything, including Infinity
        assert!(nan > inf);
        assert!(nan > zero);
        assert!(nan > neg_inf);

        assert_eq!(nan.cmp(&inf), Ordering::Greater);
        assert_eq!(inf.cmp(&nan), Ordering::Less);
    }

    #[test]
    fn test_sorting_vec() {
        // This test proves that Ord is implemented correctly for sorting algorithms
        let mut list = vec![
            OrderedFloat(1.0),
            OrderedFloat(f64::NAN),
            OrderedFloat(f64::NEG_INFINITY),
            OrderedFloat(0.0),
            OrderedFloat(f64::INFINITY),
            OrderedFloat(-5.0),
        ];

        list.sort(); // Uses Ord

        // Expected order: -Infinity, -5.0, 0.0, 1.0, Infinity, NaN
        assert_eq!(list[0].0, f64::NEG_INFINITY);
        assert_eq!(list[1].0, -5.0);
        assert_eq!(list[2].0, 0.0);
        assert_eq!(list[3].0, 1.0);
        assert_eq!(list[4].0, f64::INFINITY);
        assert!(list[5].0.is_nan());
    }

    #[test]
    fn test_hash_zeros() {
        let zero = OrderedFloat(0.0);
        let neg_zero = OrderedFloat(-0.0);

        // Verify equality logic first
        assert_eq!(zero, neg_zero);

        // Verify hash consistency
        let mut s1 = DefaultHasher::new();
        zero.hash(&mut s1);
        let h1 = s1.finish();

        let mut s2 = DefaultHasher::new();
        neg_zero.hash(&mut s2);
        let h2 = s2.finish();

        assert_eq!(h1, h2, "0.0 and -0.0 must hash to the same value");
    }

    #[test]
    fn test_hash_nan() {
        let nan1 = OrderedFloat(f64::NAN);
        let nan2 = OrderedFloat(f64::NAN);

        let mut s1 = DefaultHasher::new();
        nan1.hash(&mut s1);
        let h1 = s1.finish();

        let mut s2 = DefaultHasher::new();
        nan2.hash(&mut s2);
        let h2 = s2.finish();

        assert_eq!(h1, h2, "All NaNs must hash to the same value");
    }

    #[test]
    fn test_hash_map_usage() {
        let mut set = HashSet::new();

        // Insert values
        set.insert(OrderedFloat(0.0));
        set.insert(OrderedFloat(f64::NAN));
        set.insert(OrderedFloat(1.5));

        // Look up using equivalent but different representations
        assert!(set.contains(&OrderedFloat(-0.0)));
        assert!(set.contains(&OrderedFloat(f64::NAN)));
        assert!(set.contains(&OrderedFloat(1.5)));

        assert!(!set.contains(&OrderedFloat(2.0)));
    }
}
