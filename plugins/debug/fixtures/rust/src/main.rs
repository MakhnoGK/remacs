fn total(prices: &[u64]) -> u64 {
    let sum: u64 = prices.iter().sum();
    sum - sum * 15 / 100
}

fn main() {
    println!("total {}", total(&[990, 1300]));
}

#[cfg(test)]
mod tests {
    #[test]
    fn total_takes_the_discount_off() {
        let total = super::total(&[1000]);
        assert_eq!(total, 850);
    }
}
