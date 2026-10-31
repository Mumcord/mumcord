import type { ReactNode } from "react";

/** Logo, title and one line below it; shared by every state of the update dialog. */
export function DialogHeader(props: {
  titleId: string;
  title: string;
  subtitle: string;
  badge?: ReactNode;
}) {
  return (
    <header className="upd-header">
      <img className="upd-logo" src={`${import.meta.env.BASE_URL}favicon.png`} alt="" />
      <div className="upd-header__text">
        <h2 id={props.titleId} className="upd-title">
          {props.title}
        </h2>
        <p className="upd-subtitle">{props.subtitle}</p>
      </div>
      {props.badge}
    </header>
  );
}

export function Badge({ children }: { children: ReactNode }) {
  return <span className="upd-badge">{children}</span>;
}
