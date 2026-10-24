/**
 * Shortens device names for display. ALSA's default device carries a long suffix such as
 * "Default ALSA Output (currently PipeWire Media Server)", which only adds noise.
 */
export function readableDeviceName(name: string): string {
  const short = name.replace(/\s*\(currently [^)]*\)\s*$/i, "").trim();
  return short === "" ? name : short;
}
