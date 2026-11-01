//! `mumcord-plugin preview`: starts the plugin in the mock host of `test`, optionally plays the
//! events of a scenario, and renders the UI trees it shows into one static HTML page
//! (FR-6.71). The page approximates Mumcord's components at the slot sizes of FR-6.40 and
//! follows the system's light or dark theme; it contains no scripts and loads nothing.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use mumcord_plugins::api::theme::TOKENS;
use mumcord_plugins::wit::ui::{
    Align, Animation, ButtonKind, Component, Direction, Node, TextStyle,
};

use crate::test::{PluginFiles, Scenario, Session, SlotSpec};

/// Time for the last events to reach the plugin and its trees to come back.
const SETTLE: Duration = Duration::from_millis(300);
/// Deeper trees are cut off, and every node renders at most once, so a broken tree (cycles,
/// shared children) cannot hang the preview.
const MAX_DEPTH: usize = 64;

#[derive(clap::Args)]
pub struct Args {
    /// A built `.mcplugin` package or an unpacked plugin folder.
    pub plugin: PathBuf,
    /// Play the events and waits of this scenario first (its expectations are not checked).
    #[arg(long)]
    pub scenario: Option<PathBuf>,
    /// The HTML file to write.
    #[arg(long, default_value = "preview.html")]
    pub out: PathBuf,
}

pub fn run(args: Args) -> Result<()> {
    let plugin = PluginFiles::open(&args.plugin)?;
    let scenario = match &args.scenario {
        Some(path) => Scenario::read(path)?,
        None => Scenario::default(),
    };
    let mut session = Session::start(&plugin, &scenario)?;
    session.play(&scenario.steps)?;
    std::thread::sleep(SETTLE);
    let trees = session.trees();
    let dialog = session.dialog_title();
    session.stop();
    if trees.is_empty() {
        bail!("the plugin shows no UI");
    }
    let page = render_page(&plugin, &trees, dialog.as_deref());
    std::fs::write(&args.out, page)
        .with_context(|| format!("could not write {}", args.out.display()))?;
    println!("{}", args.out.display());
    Ok(())
}

pub fn render_page(
    plugin: &PluginFiles,
    trees: &[(SlotSpec, Vec<Node>)],
    dialog: Option<&str>,
) -> String {
    let name = escape(&plugin.manifest.name);
    let mut body = String::new();
    for (slot, nodes) in trees {
        let (label, class) = slot_label(slot, dialog);
        let mut content = String::new();
        Renderer {
            plugin,
            nodes,
            seen: vec![false; nodes.len()],
            out: &mut content,
        }
        .node(0, 0);
        let _ = writeln!(
            body,
            "<section><h2>{}</h2><div class=\"slot {class}\">{content}</div></section>",
            escape(&label)
        );
    }
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; style-src 'unsafe-inline'\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{name} preview</title>\n<style>{STYLE}</style></head>\n\
         <body><h1>{name} preview</h1>\n{body}</body></html>\n"
    )
}

fn slot_label(slot: &SlotSpec, dialog: Option<&str>) -> (String, &'static str) {
    match slot {
        SlotSpec::Panel => ("Panel".into(), "panel"),
        SlotSpec::MemberBadge(s) => (format!("Member badge (session {s})"), "badge"),
        SlotSpec::ProfileCard(s) => (format!("Profile card (session {s})"), "profile-card"),
        SlotSpec::ChatCard(id) => (format!("Chat card {id}"), "chat-card"),
        SlotSpec::ChatDecoration(s) => (format!("Chat decoration (session {s})"), "decoration"),
        SlotSpec::Settings => ("Settings".into(), "form"),
        SlotSpec::Dialog => (
            match dialog {
                Some(title) => format!("Dialog: {title}"),
                None => "Dialog (closed)".into(),
            },
            "form",
        ),
    }
}

struct Renderer<'a> {
    plugin: &'a PluginFiles,
    nodes: &'a [Node],
    seen: Vec<bool>,
    out: &'a mut String,
}

impl Renderer<'_> {
    fn node(&mut self, index: u32, depth: usize) {
        let Some(node) = self.nodes.get(index as usize) else {
            return;
        };
        if depth > MAX_DEPTH || std::mem::replace(&mut self.seen[index as usize], true) {
            return;
        }
        let anim = node
            .animation
            .map(|a| format!(" data-animation=\"{}\"", animation(a)))
            .unwrap_or_default();
        match &node.component {
            Component::Stack(s) => {
                let direction = match s.direction {
                    Direction::Row => "row",
                    Direction::Column => "column",
                };
                let align = match s.align {
                    Align::Start => "flex-start",
                    Align::Center => "center",
                    Align::End => "flex-end",
                    Align::Stretch => "stretch",
                };
                let _ = write!(
                    self.out,
                    "<div class=\"stack\"{anim} style=\"flex-direction:{direction};gap:{}px;align-items:{align}\">",
                    s.gap.min(32)
                );
                self.children(node, depth, false);
                self.out.push_str("</div>");
            }
            Component::List(l) => {
                let _ = write!(self.out, "<ul{anim} aria-label=\"{}\">", escape(&l.label));
                self.children(node, depth, true);
                self.out.push_str("</ul>");
            }
            Component::Text(t) => {
                let style = match t.style {
                    TextStyle::Body => "body",
                    TextStyle::Caption => "caption",
                    TextStyle::Heading => "heading",
                    TextStyle::Monospace => "mono",
                };
                let _ = write!(
                    self.out,
                    "<p class=\"{style}\"{anim}{}>{}</p>",
                    color(t.color.as_deref()),
                    escape(&t.text)
                );
            }
            Component::Image(i) => {
                let size = format!("width=\"{}\" height=\"{}\"", i.width, i.height);
                self.image(&i.asset, &i.alt, &size, &anim);
            }
            Component::Sprite(s) => {
                let style = format!(
                    "width:{}px;height:{}px;object-fit:none;object-position:-{}px 0",
                    s.frame_width,
                    s.frame_height,
                    u32::from(s.still_frame) * u32::from(s.frame_width)
                );
                self.image(&s.asset, &s.alt, &format!("style=\"{style}\""), &anim);
            }
            Component::Meter(m) => {
                let _ = write!(
                    self.out,
                    "<label{anim}>{} <meter min=\"0\" max=\"1\" value=\"{}\"{}></meter></label>",
                    escape(&m.label),
                    m.value.clamp(0.0, 1.0),
                    color(m.color.as_deref())
                );
            }
            Component::Button(b) => {
                let kind = match b.kind {
                    ButtonKind::Primary => "primary",
                    ButtonKind::Secondary => "secondary",
                    ButtonKind::Danger => "danger",
                };
                let _ = write!(
                    self.out,
                    "<button class=\"{kind}\"{anim}{}>{}</button>",
                    disabled(b.disabled),
                    escape(&b.label)
                );
            }
            Component::IconButton(b) => {
                let _ = write!(
                    self.out,
                    "<button class=\"icon\"{anim} aria-label=\"{}\" title=\"{}\"{}>",
                    escape(&b.label),
                    escape(&b.label),
                    disabled(b.disabled)
                );
                self.image(&b.icon, "", "width=\"16\" height=\"16\"", "");
                self.out.push_str("</button>");
            }
            Component::Chip(c) => {
                let _ = write!(
                    self.out,
                    "<span class=\"chip\"{anim}{}>{}</span>",
                    color(c.color.as_deref()),
                    escape(&c.text)
                );
            }
            Component::Divider => self.out.push_str("<hr>"),
            Component::Progress(p) => {
                let value = p
                    .value
                    .map(|v| format!(" max=\"1\" value=\"{}\"", v.clamp(0.0, 1.0)))
                    .unwrap_or_default();
                let _ = write!(
                    self.out,
                    "<label{anim}>{} <progress{value}></progress></label>",
                    escape(&p.label)
                );
            }
            Component::Toggle(t) => {
                let checked = if t.value { " checked" } else { "" };
                let _ = write!(
                    self.out,
                    "<label{anim}><input type=\"checkbox\"{checked}{}> {}</label>",
                    disabled(t.disabled),
                    escape(&t.label)
                );
            }
            Component::Select(s) => {
                let _ = write!(
                    self.out,
                    "<label{anim}>{} <select{}>",
                    escape(&s.label),
                    disabled(s.disabled)
                );
                for option in &s.options {
                    let selected = if s.selected.as_ref() == Some(&option.value) {
                        " selected"
                    } else {
                        ""
                    };
                    let _ = write!(
                        self.out,
                        "<option{selected}>{}</option>",
                        escape(&option.label)
                    );
                }
                self.out.push_str("</select></label>");
            }
            Component::Slider(s) => {
                let _ = write!(
                    self.out,
                    "<label{anim}>{} <input type=\"range\" min=\"{}\" max=\"{}\" step=\"{}\" value=\"{}\"{}></label>",
                    escape(&s.label),
                    s.min,
                    s.max,
                    s.step,
                    s.value,
                    disabled(s.disabled)
                );
            }
            Component::TextInput(t) => {
                let _ = write!(
                    self.out,
                    "<label{anim}>{} <input type=\"text\" value=\"{}\" placeholder=\"{}\" maxlength=\"{}\"{}></label>",
                    escape(&t.label),
                    escape(&t.value),
                    escape(&t.placeholder),
                    t.max_bytes,
                    disabled(t.disabled)
                );
            }
            Component::Password(p) => {
                let _ = write!(
                    self.out,
                    "<label{anim}>{} <input type=\"password\"{}></label>",
                    escape(&p.label),
                    disabled(p.disabled)
                );
            }
            Component::FilePicker(f) => {
                let _ = write!(
                    self.out,
                    "<label{anim}>{} <input type=\"file\" accept=\"{}\"{}></label>",
                    escape(&f.label),
                    escape(&f.accept.join(",")),
                    disabled(f.disabled)
                );
            }
            Component::Countdown(c) => {
                let _ = write!(
                    self.out,
                    "<span class=\"countdown\"{anim}>{} (until Unix time {})</span>",
                    escape(&c.label),
                    c.until
                );
            }
        }
    }

    fn children(&mut self, node: &Node, depth: usize, rows: bool) {
        for &child in &node.children {
            if rows {
                self.out.push_str("<li>");
            }
            self.node(child, depth + 1);
            if rows {
                self.out.push_str("</li>");
            }
        }
    }

    /// Inlines a package asset as a data URL; a missing or unknown asset shows its alt text.
    fn image(&mut self, asset: &str, alt: &str, attrs: &str, anim: &str) {
        let mime = match asset.rsplit_once('.').map(|(_, ext)| ext) {
            Some("svg") => Some("image/svg+xml"),
            Some("png") => Some("image/png"),
            Some("webp") => Some("image/webp"),
            Some("gif") => Some("image/gif"),
            Some("jpg" | "jpeg") => Some("image/jpeg"),
            _ => None,
        };
        match mime.zip(self.plugin.asset(asset)) {
            Some((mime, bytes)) => {
                let _ = write!(
                    self.out,
                    "<img{anim} src=\"data:{mime};base64,{}\" alt=\"{}\" {attrs}>",
                    base64(&bytes),
                    escape(alt)
                );
            }
            None => {
                let _ = write!(
                    self.out,
                    "<span class=\"missing\"{anim} title=\"{}\">{}</span>",
                    escape(asset),
                    escape(if alt.is_empty() { asset } else { alt })
                );
            }
        }
    }
}

fn animation(a: Animation) -> &'static str {
    match a {
        Animation::Bob => "bob",
        Animation::Hop => "hop",
        Animation::Wave => "wave",
        Animation::Nap => "nap",
        Animation::Sparkle => "sparkle",
        Animation::Shake => "shake",
        Animation::Fade => "fade",
    }
}

/// Only theme tokens are colours; anything else keeps the default colour.
fn color(token: Option<&str>) -> String {
    match token {
        Some(token) if TOKENS.contains(&token) => format!(" style=\"color:var(--{token})\""),
        _ => String::new(),
    }
}

fn disabled(disabled: bool) -> &'static str {
    if disabled { " disabled" } else { "" }
}

/// Everything from the plugin is untrusted text.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

const STYLE: &str = "
:root { color-scheme: light dark;
  --text:#17113a; --text-muted:#5c5677; --accent:#6a40e0; --danger:#b01c1c; --warning:#9a6200;
  --success:#1d7a3a; --surface:#ffffff; --surface-raised:#eee9f8; --border:#d6cfe8; --page:#f5f2fb; }
@media (prefers-color-scheme: dark) { :root {
  --text:#f6f0e8; --text-muted:#b7aecf; --accent:#a78bff; --danger:#ff8f8f; --warning:#ffc46b;
  --success:#7fdc9a; --surface:#1a1342; --surface-raised:#231b55; --border:#3a3070; --page:#0e0a26; } }
body { margin:0; padding:16px; background:var(--page); color:var(--text);
  font:14px/1.4 system-ui, sans-serif; }
h1 { font-size:18px; } h2 { font-size:13px; color:var(--text-muted); font-weight:600; }
section { margin-bottom:24px; }
.slot { background:var(--surface); border:1px solid var(--border); border-radius:8px; padding:8px;
  overflow:auto; box-sizing:border-box; }
.panel { width:260px; height:160px; } .profile-card { width:300px; height:220px; }
.chat-card { width:440px; max-width:100%; height:160px; } .form { width:420px; max-width:100%; }
.badge { width:24px; height:24px; padding:0; } .decoration { width:16px; height:16px; padding:0; }
.stack { display:flex; } p { margin:0; } .heading { font-weight:700; font-size:16px; }
.caption { font-size:12px; color:var(--text-muted); } .mono { font-family:ui-monospace, monospace; }
ul { list-style:none; margin:0; padding:0; } li { padding:2px 0; border-bottom:1px solid var(--border); }
button { border-radius:6px; border:1px solid var(--border); padding:4px 10px; font:inherit;
  background:var(--surface-raised); color:var(--text); }
button.primary { background:var(--accent); color:var(--surface); border-color:var(--accent); }
button.danger { color:var(--danger); border-color:var(--danger); }
.chip { display:inline-block; padding:0 8px; border-radius:999px; background:var(--surface-raised); }
.missing { border:1px dashed var(--border); color:var(--text-muted); padding:0 4px; }
hr { border:0; border-top:1px solid var(--border); width:100%; }
img, label { max-width:100%; }
";

#[cfg(test)]
mod tests {
    use mumcord_plugins::wit::ui::{Button, Stack, Text};

    use super::*;

    fn node(component: Component, children: Vec<u32>) -> Node {
        Node {
            component,
            animation: None,
            children,
        }
    }

    #[test]
    fn plugin_text_is_escaped_and_trees_render_depth_first() {
        let dir = tempfile::tempdir().expect("temp dir");
        let plugin = crate::test::tests::files(dir.path());
        let nodes = vec![
            node(
                Component::Stack(Stack {
                    direction: Direction::Column,
                    gap: 200,
                    align: Align::Start,
                }),
                vec![1, 2, 9],
            ),
            node(
                Component::Text(Text {
                    text: "<script>alert(1)</script>".into(),
                    style: TextStyle::Heading,
                    color: Some("red; background:url(x)".into()),
                }),
                vec![],
            ),
            node(
                Component::Button(Button {
                    label: "Wave".into(),
                    action: "wave".into(),
                    kind: ButtonKind::Primary,
                    disabled: false,
                }),
                vec![],
            ),
        ];
        let page = render_page(&plugin, &[(SlotSpec::Panel, nodes)], None);
        assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!page.contains("<script>"));
        assert!(!page.contains("url(x)"));
        assert!(page.contains("gap:32px"));
        let text = page.find("&lt;script").expect("text");
        let button = page.find(">Wave</button>").expect("button");
        assert!(text < button);
    }

    #[test]
    fn cyclic_trees_render_each_node_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let plugin = crate::test::tests::files(dir.path());
        let looped = vec![node(
            Component::Stack(Stack {
                direction: Direction::Row,
                gap: 0,
                align: Align::Center,
            }),
            vec![0, 0],
        )];
        let page = render_page(&plugin, &[(SlotSpec::Panel, looped)], None);
        assert_eq!(page.matches("class=\"stack\"").count(), 1);
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xff, 0xfe]), "//4=");
    }
}
