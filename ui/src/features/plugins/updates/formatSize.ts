/** A download size as "340 kB" or "1.2 MB" in the UI language. */
export function formatSize(bytes: number, language: string): string {
  const megabytes = bytes / 1_000_000;
  const big = megabytes >= 1;
  return new Intl.NumberFormat(language, {
    style: "unit",
    unit: big ? "megabyte" : "kilobyte",
    unitDisplay: "short",
    maximumFractionDigits: 1,
  }).format(big ? megabytes : bytes / 1000);
}
