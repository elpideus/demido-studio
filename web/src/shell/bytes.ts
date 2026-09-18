/**
 * Bytes as a person reads a model's size.
 *
 * One place, because the model list and the download queue both print a
 * model's size, and a row that says 4.8 GiB in one window and 5.1 GB in the
 * other is two numbers for one file.
 */
export function size(bytes: number): string {
  const gib = bytes / (1024 * 1024 * 1024)
  return gib >= 1 ? `${gib.toFixed(1)} GiB` : `${(bytes / (1024 * 1024)).toFixed(1)} MiB`
}
