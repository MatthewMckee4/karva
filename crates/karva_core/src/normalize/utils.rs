use std::rc::Rc;

/// Computes the cartesian product of multiple Rc slices.
pub fn cartesian_product_rc<T: Clone>(vecs: &[Rc<[T]>]) -> Vec<Rc<[T]>> {
    if vecs.is_empty() {
        return vec![Rc::from(Vec::new().into_boxed_slice())];
    }

    if vecs.len() == 1 {
        return vecs[0]
            .iter()
            .map(|item| Rc::from(vec![item.clone()].into_boxed_slice()))
            .collect();
    }

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

    result
        .into_iter()
        .map(|v| Rc::from(v.into_boxed_slice()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cartesian_product_empty() {
        let input: Vec<Rc<[i32]>> = vec![];
        let result = cartesian_product_rc(&input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 0);
    }

    #[test]
    fn test_cartesian_product_single() {
        let input = vec![Rc::from(vec![1, 2, 3].into_boxed_slice())];
        let result = cartesian_product_rc(&input);
        assert_eq!(result.len(), 3);
        assert_eq!(&*result[0], &[1]);
        assert_eq!(&*result[1], &[2]);
        assert_eq!(&*result[2], &[3]);
    }

    #[test]
    fn test_cartesian_product_two() {
        let input = vec![
            Rc::from(vec![1, 2].into_boxed_slice()),
            Rc::from(vec![3, 4].into_boxed_slice()),
        ];
        let result = cartesian_product_rc(&input);
        assert_eq!(result.len(), 4);
        assert_eq!(&*result[0], &[1, 3]);
        assert_eq!(&*result[1], &[1, 4]);
        assert_eq!(&*result[2], &[2, 3]);
        assert_eq!(&*result[3], &[2, 4]);
    }

    #[test]
    fn test_cartesian_product_three() {
        let input = vec![
            Rc::from(vec![1, 2].into_boxed_slice()),
            Rc::from(vec![3].into_boxed_slice()),
            Rc::from(vec![4, 5].into_boxed_slice()),
        ];
        let result = cartesian_product_rc(&input);
        assert_eq!(result.len(), 4);
        assert_eq!(&*result[0], &[1, 3, 4]);
        assert_eq!(&*result[1], &[1, 3, 5]);
        assert_eq!(&*result[2], &[2, 3, 4]);
        assert_eq!(&*result[3], &[2, 3, 5]);
    }
}
