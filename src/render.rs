use crate::catalog::{Dataset, valid_id};
use anyhow::{Result, ensure};
use fontdue::{Font, FontSettings};
use html2text::render::RichAnnotation;
use unicode_segmentation::UnicodeSegmentation;

pub struct Card {
    pub png: Vec<u8>,
    pub alt: String,
    pub height: u32,
}
pub const WIDTH: u32 = 1200;
const MARGIN: f32 = 72.;
const MEASURE: f32 = WIDTH as f32 - 2. * MARGIN;
const BODY: f32 = 32.;
const LEADING: f32 = BODY * 1.5;
const TITLE: f32 = 54.;
const CONTENT_HEIGHT: f32 = 1470.;
const INK: [u8; 3] = [24, 37, 51];
const MUTED: [u8; 3] = [65, 85, 102];
const BLUE: [u8; 3] = [20, 105, 145];
const PAPER: [u8; 3] = [248, 248, 248];

struct Fonts {
    regular: Font,
    semibold: Font,
}
impl Fonts {
    fn load() -> Result<Self> {
        let load = |bytes: &'static [u8]| {
            Font::from_bytes(bytes, FontSettings::default()).map_err(|e| anyhow::anyhow!(e))
        };
        Ok(Self {
            regular: load(include_bytes!("../assets/NotoSans-Regular.ttf"))?,
            semibold: load(include_bytes!("../assets/NotoSans-SemiBold.ttf"))?,
        })
    }
    fn get(&self, bold: bool) -> &Font {
        if bold { &self.semibold } else { &self.regular }
    }
}
#[derive(Clone, Debug, Default)]
struct Span {
    text: String,
    link: Option<String>,
    bold: bool,
}
impl Span {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}
#[derive(Clone)]
struct Row {
    spans: Vec<Span>,
    x: f32,
    y: f32,
    size: f32,
    color: [u8; 3],
}
struct Block {
    rows: Vec<Row>,
    height: f32,
    alt: String,
    panel: bool,
}

fn compact_url(value: &str) -> String {
    let Ok(url) = reqwest::Url::parse(value) else {
        return value.to_string();
    };
    if url.host_str() == Some("data.cityofchicago.org")
        && url.query().is_none()
        && url.fragment().is_none()
    {
        let segments: Vec<_> = url.path_segments().into_iter().flatten().collect();
        if let Some(id) = segments.last().filter(|id| valid_id(id)).or_else(|| {
            if segments.last() == Some(&"about_data") {
                segments.iter().rev().nth(1).filter(|id| valid_id(id))
            } else {
                None
            }
        }) {
            return format!("data.cityofchicago.org/d/{id}");
        }
    }
    value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .unwrap_or(value)
        .to_string()
}
fn spans_from_text(text: &str, link: Option<String>, bold: bool) -> Vec<Span> {
    if let Some(link) = link {
        let label = if text.trim().starts_with("https://") || text.trim().starts_with("http://") {
            compact_url(text.trim())
        } else {
            text.to_string()
        };
        return vec![Span {
            text: label,
            link: Some(link),
            bold,
        }];
    }
    let mut spans = Vec::new();
    for part in text.split_inclusive(char::is_whitespace) {
        let token = part.trim_end_matches(char::is_whitespace);
        let bare = token.trim_end_matches(['.', ',', ';', ')', ']']);
        if (bare.starts_with("https://") || bare.starts_with("http://"))
            && reqwest::Url::parse(bare).is_ok()
        {
            spans.push(Span {
                text: compact_url(bare),
                link: Some(bare.into()),
                bold,
            });
            spans.push(Span {
                text: part[bare.len()..].into(),
                bold,
                ..Span::default()
            });
        } else {
            spans.push(Span {
                text: part.into(),
                bold,
                ..Span::default()
            });
        }
    }
    spans
}
fn description(html: &str) -> Result<Vec<Vec<Span>>> {
    // Socrata descriptions mix HTML with plaintext blank-line paragraphs.
    let html = html.replace("\r\n", "\n").replace("\n\n", "<br><br>");
    let lines = html2text::from_read_rich(html.as_bytes(), 100_000)?;
    let mut paragraphs = vec![Vec::new()];
    for line in lines {
        let strings: Vec<_> = line.tagged_strings().collect();
        if strings.iter().all(|s| s.s.trim().is_empty()) {
            if !paragraphs.last().unwrap().is_empty() {
                paragraphs.push(Vec::new());
            }
            continue;
        }
        let p = paragraphs.last_mut().unwrap();
        if !p.is_empty() {
            p.push(Span::text(" "));
        }
        for s in strings {
            let link = s.tag.iter().find_map(|a| {
                if let RichAnnotation::Link(url) = a {
                    Some(url.clone())
                } else {
                    None
                }
            });
            let bold = s
                .tag
                .iter()
                .any(|a| matches!(a, RichAnnotation::Strong | RichAnnotation::Emphasis));
            p.extend(spans_from_text(&s.s, link, bold));
        }
    }
    paragraphs.retain(|p| !p.is_empty());
    Ok(paragraphs)
}
fn accessible(spans: &[Span]) -> String {
    let mut text = spans.iter().map(|s| s.text.as_str()).collect::<String>();
    let mut links = Vec::new();
    for span in spans {
        if let Some(url) = &span.link
            && !links.contains(url)
        {
            links.push(url.clone());
        }
    }
    for url in links {
        text.push_str("\nLink: ");
        text.push_str(&url);
    }
    text
}
fn width(spans: &[Span], fonts: &Fonts, size: f32) -> f32 {
    spans
        .iter()
        .map(|s| {
            s.text
                .chars()
                .map(|c| fonts.get(s.bold).metrics(c, size).advance_width)
                .sum::<f32>()
        })
        .sum()
}
fn push_char(spans: &mut Vec<Span>, c: char, style: &Span) {
    if let Some(last) = spans
        .last_mut()
        .filter(|s| s.bold == style.bold && s.link == style.link)
    {
        last.text.push(c);
    } else {
        spans.push(Span {
            text: c.to_string(),
            link: style.link.clone(),
            bold: style.bold,
        });
    }
}
fn wrap(spans: &[Span], fonts: &Fonts, size: f32, measure: f32) -> Vec<Vec<Span>> {
    // Group across HTML span boundaries so punctuation remains with its word.
    let mut words: Vec<Vec<Span>> = Vec::new();
    let mut word = Vec::new();
    for span in spans {
        for c in span.text.chars() {
            if c.is_whitespace() {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            } else {
                push_char(&mut word, c, span);
            }
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    let space = fonts.regular.metrics(' ', size).advance_width;
    let mut lines = Vec::new();
    let mut line = Vec::new();
    let mut used = 0.;
    for word in words {
        let w = width(&word, fonts, size);
        if !line.is_empty() && used + space + w > measure {
            lines.push(std::mem::take(&mut line));
            used = 0.;
        }
        if !line.is_empty() {
            line.push(Span::text(" "));
            used += space;
        }
        for span in word {
            for c in span.text.chars() {
                let advance = fonts.get(span.bold).metrics(c, size).advance_width;
                if used + advance > measure && !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    used = 0.;
                }
                push_char(&mut line, c, &span);
                used += advance;
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
fn text_block(
    spans: Vec<Span>,
    fonts: &Fonts,
    size: f32,
    leading: f32,
    gap: f32,
    balance: bool,
) -> Block {
    let mut lines = wrap(&spans, fonts, size, MEASURE);
    if balance && lines.len() > 1 {
        let count = lines.len();
        let mut best = lines.clone();
        let mut score = f32::MAX;
        for step in 0..=40 {
            let candidate = wrap(&spans, fonts, size, MEASURE * (1. - step as f32 * 0.01));
            if candidate.len() != count {
                continue;
            }
            let widths: Vec<_> = candidate.iter().map(|s| width(s, fonts, size)).collect();
            let spread = widths.iter().copied().fold(f32::MIN, f32::max)
                - widths.iter().copied().fold(f32::MAX, f32::min);
            if spread < score {
                score = spread;
                best = candidate;
            }
        }
        lines = best;
    }
    let height = lines.len() as f32 * leading + gap;
    Block {
        rows: lines
            .into_iter()
            .enumerate()
            .map(|(i, spans)| Row {
                spans,
                x: MARGIN,
                y: i as f32 * leading,
                size,
                color: INK,
            })
            .collect(),
        height,
        alt: accessible(&spans),
        panel: false,
    }
}
fn metadata(d: &Dataset, fonts: &Fonts) -> Result<Block> {
    let fields = [
        ("Data Owner", d.data_owner.clone()),
        ("Category", d.category.clone()),
        ("Date Created", d.date()?),
        ("Dataset Owner", d.dataset_owner.clone()),
    ];
    let mut rows = Vec::new();
    let mut y = 28.;
    for pair in fields.chunks(2) {
        let mut height: f32 = 0.;
        for (col, (label, value)) in pair.iter().enumerate() {
            let x = MARGIN + 28. + col as f32 * 520.;
            rows.push(Row {
                spans: vec![Span::text(*label)],
                x,
                y,
                size: 24.,
                color: MUTED,
            });
            let lines = wrap(
                &[Span {
                    text: value.clone(),
                    bold: true,
                    ..Span::default()
                }],
                fonts,
                28.,
                464.,
            );
            height = height.max(38. + lines.len() as f32 * 40.);
            for (i, spans) in lines.into_iter().enumerate() {
                rows.push(Row {
                    spans,
                    x,
                    y: y + 38. + i as f32 * 40.,
                    size: 28.,
                    color: INK,
                });
            }
        }
        y += height + 24.;
    }
    Ok(Block {
        rows,
        height: y + 12.,
        alt: fields
            .iter()
            .map(|(label, value)| format!("{label}: {value}"))
            .collect::<Vec<_>>()
            .join("\n"),
        panel: true,
    })
}
struct Canvas {
    pixels: Vec<u8>,
    height: u32,
}
impl Canvas {
    fn new(height: u32) -> Self {
        Self {
            pixels: PAPER.repeat((WIDTH * height) as usize),
            height,
        }
    }
    fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: [u8; 3]) {
        for yy in y..(y + h).min(self.height) {
            for xx in x..(x + w).min(WIDTH) {
                let i = ((yy * WIDTH + xx) * 3) as usize;
                self.pixels[i..i + 3].copy_from_slice(&color);
            }
        }
    }
    fn row(&mut self, row: &Row, offset: f32, fonts: &Fonts) {
        let mut x = row.x;
        for span in &row.spans {
            let font = fonts.get(span.bold);
            let color = if span.link.is_some() { BLUE } else { row.color };
            for c in span.text.chars() {
                let (m, bitmap) = font.rasterize(c, row.size);
                let baseline = offset + row.y + row.size;
                for r in 0..m.height {
                    for col in 0..m.width {
                        let xx = x as i32 + m.xmin + col as i32;
                        let yy = baseline as i32 - m.ymin - m.height as i32 + r as i32;
                        if xx < 0 || xx >= WIDTH as i32 || yy < 0 || yy >= self.height as i32 {
                            continue;
                        }
                        let index = (yy as usize * WIDTH as usize + xx as usize) * 3;
                        let a = bitmap[r * m.width + col] as u32;
                        for (k, channel) in color.iter().enumerate() {
                            self.pixels[index + k] = ((*channel as u32 * a
                                + self.pixels[index + k] as u32 * (255 - a))
                                / 255) as u8;
                        }
                    }
                }
                // Draw link underlines with descender clearance, skipping glyph ink.
                if span.link.is_some() && !c.is_whitespace() {
                    for px in 0..m.advance_width.ceil() as u32 {
                        let xx = x as u32 + px;
                        let yy = (baseline + 5.) as u32;
                        let clear =
                            (yy.saturating_sub(2)..=(yy + 3).min(self.height - 1)).all(|y| {
                                xx < WIDTH
                                    && self.pixels[((y * WIDTH + xx) * 3) as usize
                                        ..((y * WIDTH + xx) * 3 + 3) as usize]
                                        == PAPER
                            });
                        if clear {
                            self.rect(xx, yy, 1, 2, color);
                        }
                    }
                }
                x += m.advance_width;
            }
        }
    }
    fn png(self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut bytes, WIDTH, self.height);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()?.write_image_data(&self.pixels)?;
        }
        ensure!(
            bytes.len() < 2_000_000,
            "Card exceeds Bluesky's 2 MB image limit"
        );
        Ok(bytes)
    }
}

pub fn cards(d: &Dataset) -> Result<Vec<Card>> {
    let fonts = Fonts::load()?;
    let mut blocks = vec![
        text_block(
            vec![Span {
                text: d.name.clone(),
                bold: true,
                ..Span::default()
            }],
            &fonts,
            TITLE,
            TITLE * 1.1,
            36.,
            true,
        ),
        metadata(d, &fonts)?,
    ];
    for spans in description(&d.description)? {
        blocks.push(text_block(spans, &fonts, BODY, LEADING, 32., false));
    }
    let mut pages: Vec<Vec<Block>> = vec![Vec::new()];
    let mut used = 0.;
    for mut block in blocks {
        if block.panel {
            block.height += 40.;
        }
        if block.height <= CONTENT_HEIGHT {
            if used + block.height > CONTENT_HEIGHT {
                pages.push(Vec::new());
                used = 0.;
            }
            used += block.height;
            pages.last_mut().unwrap().push(block);
        } else {
            ensure!(!block.panel, "Metadata is too large for a readable card");
            // Oversized paragraphs split into readable pages; never lose their link destinations.
            for chunk in block.rows.chunks(24) {
                let first = chunk[0].y;
                let mut rows = chunk.to_vec();
                for row in &mut rows {
                    row.y -= first;
                }
                let height = rows.last().unwrap().y + LEADING + 32.;
                if used + height > CONTENT_HEIGHT {
                    pages.push(Vec::new());
                    used = 0.;
                }
                let spans = rows
                    .iter()
                    .flat_map(|r| r.spans.iter().cloned().chain([Span::text(" ")]))
                    .collect::<Vec<_>>();
                pages.last_mut().unwrap().push(Block {
                    rows,
                    height,
                    alt: accessible(&spans),
                    panel: false,
                });
                used += height;
            }
        }
        ensure!(
            pages.len() <= 4,
            "Description needs more than four readable cards; review this dataset manually"
        );
    }
    let count = pages.len();
    pages
        .into_iter()
        .enumerate()
        .map(|(i, blocks)| {
            let content: f32 = blocks.iter().map(|b| b.height).sum();
            let height = (content as u32 + 220).max(600);
            let mut canvas = Canvas::new(height);
            canvas.rect(0, 0, WIDTH, 10, BLUE);
            canvas.row(
                &Row {
                    spans: vec![Span {
                        text: "Chicago Data Portal".into(),
                        bold: true,
                        ..Span::default()
                    }],
                    x: MARGIN,
                    y: 44.,
                    size: 24.,
                    color: BLUE,
                },
                0.,
                &fonts,
            );
            let mut y = 112.;
            let mut alt = String::new();
            for block in blocks {
                if block.panel {
                    canvas.rect(
                        MARGIN as u32,
                        y as u32,
                        MEASURE as u32,
                        (block.height - 40.) as u32,
                        [237, 241, 244],
                    );
                }
                for row in &block.rows {
                    canvas.row(row, y, &fonts);
                }
                y += block.height;
                alt.push_str(&block.alt);
                alt.push_str("\n\n");
            }
            canvas.row(
                &Row {
                    spans: vec![Span::text(format!("New dataset  /  {}", d.id))],
                    x: MARGIN,
                    y: height as f32 - 64.,
                    size: 23.,
                    color: MUTED,
                },
                0.,
                &fonts,
            );
            canvas.row(
                &Row {
                    spans: vec![Span::text(format!("{} / {}", i + 1, count))],
                    x: 1040.,
                    y: height as f32 - 64.,
                    size: 23.,
                    color: MUTED,
                },
                0.,
                &fonts,
            );
            Ok(Card {
                png: canvas.png()?,
                alt: alt.trim().into(),
                height,
            })
        })
        .collect()
}
pub fn post_text(d: &Dataset) -> Result<String> {
    let tail = format!(
        "\n\nData Owner: {}\nCategory: {}\nDate Created: {}\nDataset Owner: {}\n\n{}",
        d.data_owner,
        d.category,
        d.date()?,
        d.dataset_owner,
        d.url()
    );
    let full = format!("New Chicago dataset\n{}{}", d.name, tail);
    if full.graphemes(true).count() <= 300 && full.len() <= 3000 {
        return Ok(full);
    }
    let tail = format!("\n\nDetails in image.\n{}", d.url());
    let head = "New Chicago dataset\n";
    let budget = 300 - head.graphemes(true).count() - tail.graphemes(true).count() - 1;
    let title: String = d.name.graphemes(true).take(budget).collect();
    let text = format!("{head}{title}…{tail}");
    ensure!(text.len() <= 3000, "Post exceeds UTF-8 limit");
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_render_as_labels_without_markdown_and_keep_destinations() {
        let source = "Before <a href=\"https://example.org/full?x=1&amp;y=2\"><strong>Housing &amp; permits</strong></a>, after.\n\nMap: https://data.cityofchicago.org/Buildings/ADU-Map/n8dk-kjjn.";
        let p = description(source).unwrap();
        assert_eq!(p.len(), 2);
        let visible = p[0].iter().map(|s| s.text.as_str()).collect::<String>();
        assert_eq!(visible, "Before Housing & permits, after.");
        assert!(!visible.contains("[1]"));
        assert!(
            p[0].iter()
                .any(|s| s.bold && s.link.as_deref() == Some("https://example.org/full?x=1&y=2"))
        );
        assert_eq!(
            p[1].iter().map(|s| s.text.as_str()).collect::<String>(),
            "Map: data.cityofchicago.org/d/n8dk-kjjn."
        );
        assert!(
            accessible(&p[1])
                .contains("https://data.cityofchicago.org/Buildings/ADU-Map/n8dk-kjjn")
        );
    }
    #[test]
    fn compact_links_preserve_meaningful_queries_and_other_hosts() {
        assert_eq!(
            compact_url("https://data.cityofchicago.org/Buildings/ADU/xbwc-ntpx/about_data"),
            "data.cityofchicago.org/d/xbwc-ntpx"
        );
        assert_eq!(
            compact_url("https://data.cityofchicago.org/d/xbwc-ntpx?x=1#section"),
            "data.cityofchicago.org/d/xbwc-ntpx?x=1#section"
        );
        assert_eq!(
            compact_url("https://example.org/some/path"),
            "example.org/some/path"
        );
    }
    #[test]
    fn wrapping_keeps_inline_punctuation_and_long_tokens_within_measure() {
        let fonts = Fonts::load().unwrap();
        let p = description("Read <a href=\"https://example.org\">this link</a>, then continue.")
            .unwrap();
        let rows = wrap(&p[0], &fonts, BODY, MEASURE);
        assert_eq!(
            rows[0].iter().map(|s| s.text.as_str()).collect::<String>(),
            "Read this link, then continue."
        );
        for row in wrap(&[Span::text("a".repeat(1000))], &fonts, BODY, MEASURE) {
            assert!(width(&row, &fonts, BODY) <= MEASURE + 0.1);
        }
    }
    #[test]
    fn multipage_text_and_full_urls_survive_pagination() {
        let fixture: crate::catalog::Page =
            serde_json::from_str(include_str!("../tests/fixtures/adu.json")).unwrap();
        let mut d = fixture
            .results
            .into_iter()
            .next()
            .unwrap()
            .dataset()
            .unwrap();
        d.description = (0..8)
            .map(|i| {
                format!(
                    "Paragraph {i}. {} <a href=\"https://example.org/source/{i}\">Source {i}</a>.",
                    "Readable description content. ".repeat(12)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let rendered = cards(&d).unwrap();
        assert!(rendered.len() > 1 && rendered.len() <= 4);
        let text = rendered
            .iter()
            .map(|c| c.alt.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for i in 0..8 {
            assert_eq!(text.matches(&format!("Paragraph {i}.")).count(), 1);
            assert!(text.contains(&format!("https://example.org/source/{i}")));
        }
        for card in rendered {
            assert!(card.height <= 1690);
        }
    }
}
