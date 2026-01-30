use std::sync::Arc;

/// Computes the cartesian product of multiple Arc slices.
///
/// Returns `Vec<Arc<[T]>>` where each Arc slice is a combination.
/// This is optimized for use with `Arc<NormalizedFixture>` to minimize cloning.
///
/// For example: `cartesian_product_arc(&[Arc::from([1,2]), Arc::from([3,4])])`
/// returns combinations: [[1,3], [1,4], [2,3], [2,4]]
pub fn cartesian_product_arc<T: Clone>(vecs: &[Arc<[T]>]) -> Vec<Arc<[T]>> {
    if vecs.is_empty() {
        return vec![Arc::from(Vec::new().into_boxed_slice())];
    }

    // Handle single-element case efficiently
    if vecs.len() == 1 {
        return vecs[0]
            .iter()
            .map(|item| Arc::from(vec![item.clone()].into_boxed_slice()))
            .collect();
    }

    // Calculate total result size to pre-allocate
    let total_combinations: usize = vecs.iter().map(|v| v.len().max(1)).product();

    let mut result = Vec::with_capacity(total_combinations);
    result.push(Vec::new());

    for vec in vecs {
        let mut new_result = Vec::with_capacity(result.len() * vec.len());
        for existing in &result {
            for item in vec.iter() {
                let mut new_combination = existing.clone();
                new_combination.push(item.clone());
                new_result.push(new_combination);
            }
        }
        result = new_result;
    }

    // Convert Vec<Vec<T>> to Vec<Arc<[T]>>
    result
        .into_iter()
        .map(|v| Arc::from(v.into_boxed_slice()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cartesian_product_empty() {
        let input: Vec<Arc<[i32]>> = vec![];
        let result = cartesian_product_arc(&input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 0);
    }

    #[test]
    fn test_cartesian_product_single() {
        let input = vec![Arc::from(vec![1, 2, 3].into_boxed_slice())];
        let result = cartesian_product_arc(&input);
        assert_eq!(result.len(), 3);
        assert_eq!(&*result[0], &[1]);
        assert_eq!(&*result[1], &[2]);
        assert_eq!(&*result[2], &[3]);
    }

    #[test]
    fn test_cartesian_product_two() {
        let input = vec![
            Arc::from(vec![1, 2].into_boxed_slice()),
            Arc::from(vec![3, 4].into_boxed_slice()),
        ];
        let result = cartesian_product_arc(&input);
        assert_eq!(result.len(), 4);
        assert_eq!(&*result[0], &[1, 3]);
        assert_eq!(&*result[1], &[1, 4]);
        assert_eq!(&*result[2], &[2, 3]);
        assert_eq!(&*result[3], &[2, 4]);
    }

    #[test]
    fn test_cartesian_product_three() {
        let input = vec![
            Arc::from(vec![1, 2].into_boxed_slice()),
            Arc::from(vec![3].into_boxed_slice()),
            Arc::from(vec![4, 5].into_boxed_slice()),
        ];
        let result = cartesian_product_arc(&input);
        assert_eq!(result.len(), 4);
        assert_eq!(&*result[0], &[1, 3, 4]);
        assert_eq!(&*result[1], &[1, 3, 5]);
        assert_eq!(&*result[2], &[2, 3, 4]);
        assert_eq!(&*result[3], &[2, 3, 5]);
    }
}
