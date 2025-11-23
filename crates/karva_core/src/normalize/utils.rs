/// Computes the cartesian product of multiple vectors.
/// For example: `cartesian_product(vec`![vec![1,2], vec![3,4]])
/// returns vec![vec![1,3], vec![1,4], vec![2,3], vec![2,4]]
pub fn cartesian_product<T: Clone>(vecs: Vec<Vec<T>>) -> Vec<Vec<T>> {
    if vecs.is_empty() {
        return vec![vec![]];
    }

    let mut result = vec![vec![]];

    for vec in vecs {
        let mut new_result = Vec::new();
        for existing in &result {
            for item in &vec {
                let mut new_combination = existing.clone();
                new_combination.push(item.clone());
                new_result.push(new_combination);
            }
        }
        result = new_result;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cartesian_product_single() {
        let result = cartesian_product(vec![vec![1, 2, 3]]);
        assert_eq!(result, vec![vec![1], vec![2], vec![3]]);
    }

    #[test]
    fn test_cartesian_product_two() {
        let result = cartesian_product(vec![vec![1, 2], vec![3, 4]]);
        assert_eq!(result, vec![vec![1, 3], vec![1, 4], vec![2, 3], vec![2, 4]]);
    }

    #[test]
    fn test_cartesian_product_three() {
        let result = cartesian_product(vec![vec![1, 2], vec![3], vec![4, 5]]);
        assert_eq!(
            result,
            vec![vec![1, 3, 4], vec![1, 3, 5], vec![2, 3, 4], vec![2, 3, 5]]
        );
    }
}
