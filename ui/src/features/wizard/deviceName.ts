/**
 * Device names straight from the OS can be long and technical. The ALSA default device, for
 * example, is called "Default ALSA Output (currently PipeWire Media Server)"; the wizard shows
 * it as "System default (PipeWire)".
 */
const ALSA_DEFAULT = /^Default ALSA (?:Output|Input) \(currently (.+)\)$/;
const SERVER_SUFFIX = / (?:Media|Sound) Server$/;

export function shortDeviceName(name: string, defaultVia: (backend: string) => string): string {
  const firstLine = (name.split("\n")[0] ?? name).trim();
  const alsaDefault = ALSA_DEFAULT.exec(firstLine);
  if (alsaDefault?.[1]) return defaultVia(alsaDefault[1].replace(SERVER_SUFFIX, ""));
  return firstLine;
}
