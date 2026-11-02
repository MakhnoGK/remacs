function total(prices: number[]): number {
  const sum = prices.reduce((a, b) => a + b, 0);
  return sum - (sum * 15) / 100;
}

console.log(`total ${total([990, 1300])}`);
