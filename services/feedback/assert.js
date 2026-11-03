// `--no-remote` keeps the checks offline, so the tests carry their own small assertion.
export function assertEquals(actual, expected) {
  const [a, b] = [JSON.stringify(actual), JSON.stringify(expected)];
  if (a !== b) throw new Error(`expected ${b}, got ${a}`);
}
