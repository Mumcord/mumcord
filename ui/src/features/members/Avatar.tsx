import "./members.css";

/** Round avatar: the person's image when known, otherwise their initial. Decorative. */
export function Avatar({
  name,
  src = null,
  talking = false,
  size = "md",
}: {
  name: string;
  src?: string | null;
  talking?: boolean;
  size?: "md" | "lg";
}) {
  return (
    <span className="avatar" data-size={size} data-talking={talking} aria-hidden="true">
      {src ? <img src={src} alt="" /> : (name.trim()[0] ?? "?").toUpperCase()}
    </span>
  );
}
