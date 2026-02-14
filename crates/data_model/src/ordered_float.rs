use std::cmp::Ordering;

/// New Type wrapper for f64 that implements the Eq and Ord traits so
/// we can use it in indexes. The f64 does not implement these traits
/// natively because of the NaN value which is not equal or comparable
/// to another NaN value. We wrap the f64 and implement the traits
/// to treat NaN == NanN as true and greater than anything other than
/// NaN.
#[derive(Debug, Clone, Copy)]
pub struct OrderedFloat(f64);

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
    use std::cmp::Ordering;

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
}
