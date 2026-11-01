import { mockPlaceholder } from "./mockPlaceholder";

/** The mock's placeholder picture (issue #163): what a screenshot of the mock UI shows. */
export function MockPicture({ label }: { label: string }) {
  const picture = mockPlaceholder();
  if (picture === null) return null;
  return <img className="media-frame__mockpicture" src={picture} alt={label} />;
}
