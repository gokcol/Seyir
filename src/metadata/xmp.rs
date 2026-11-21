//! Adobe XMP Dublin Core properties stored as RDF/XML.
use super::EditableMetadata;
use roxmltree::{Document, Node};
use std::ops::Range;

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
const EMPTY_PACKET: &[u8] = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'></rdf:RDF></x:xmpmeta>";

#[derive(Clone, Copy)]
enum Kind {
    Alternate,
    List(&'static str),
}
const FIELDS: [(&str, Kind); 5] = [
    ("title", Kind::Alternate),
    ("description", Kind::Alternate),
    ("creator", Kind::List("Seq")),
    ("rights", Kind::Alternate),
    ("subject", Kind::List("Bag")),
];

fn text(packet: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(packet)
        .map(|value| value.trim_end_matches('\0'))
        .map_err(|_| "This image has non-UTF-8 XMP metadata. It is read-only.".into())
}

fn parse(xml: &str) -> Result<Document<'_>, String> {
    Document::parse(xml)
        .map_err(|_| "The embedded XMP packet is malformed. It is read-only.".into())
}

fn root<'a, 'input>(document: &'a Document<'input>) -> Result<Node<'a, 'input>, String> {
    document
        .descendants()
        .find(|node| node.has_tag_name((RDF, "RDF")))
        .ok_or_else(|| "The embedded XMP has no RDF root. It is read-only.".into())
}

fn descriptions<'a, 'input>(
    root: Node<'a, 'input>,
) -> (&'a str, impl Iterator<Item = Node<'a, 'input>> + Clone) {
    let descriptions = root
        .children()
        .filter(|node| node.has_tag_name((RDF, "Description")));
    let subject = descriptions
        .clone()
        .find(|node| node.attribute((RDF, "about")).unwrap_or("").is_empty())
        .or_else(|| descriptions.clone().next())
        .and_then(|node| node.attribute((RDF, "about")))
        .unwrap_or("");
    (
        subject,
        descriptions.filter(move |node| node.attribute((RDF, "about")).unwrap_or("") == subject),
    )
}

fn text_value(node: Node<'_, '_>) -> String {
    node.children()
        .filter(Node::is_text)
        .filter_map(|node| node.text())
        .collect()
}

fn list_members<'a, 'input>(
    node: Node<'a, 'input>,
    name: &str,
) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children()
        .find(|child| child.has_tag_name((RDF, name)))
        .into_iter()
        .flat_map(|container| {
            container
                .children()
                .filter(|child| child.has_tag_name((RDF, "li")))
        })
}

fn value<'a, 'input: 'a>(
    descriptions: impl Iterator<Item = Node<'a, 'input>>,
    property: &str,
    kind: Kind,
) -> String {
    for desc in descriptions {
        if let Some(node) = desc
            .children()
            .find(|element| element.has_tag_name((DC, property)))
        {
            return match kind {
                Kind::Alternate => {
                    let mut items = list_members(node, "Alt");
                    items
                        .find(|entry| entry.attribute((XML, "lang")) == Some("x-default"))
                        .or_else(|| list_members(node, "Alt").next())
                        .map_or_else(|| text_value(node), text_value)
                }
                Kind::List(name) => {
                    let members: Vec<_> = list_members(node, name).map(text_value).collect();
                    if members.is_empty() {
                        text_value(node)
                    } else {
                        members.join(", ")
                    }
                }
            };
        }
        if let Some(attribute) = desc.attribute((DC, property)) {
            return attribute.into();
        }
    }
    String::new()
}

pub(super) fn read(packet: &[u8]) -> Result<EditableMetadata, String> {
    let document = parse(text(packet)?)?;
    let (_, descriptions) = descriptions(root(&document)?);
    let [title, description, author, copyright, keywords] =
        FIELDS.map(|(property, kind)| value(descriptions.clone(), property, kind));
    Ok(EditableMetadata {
        title,
        description,
        author,
        copyright,
        keywords,
    })
}

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

fn validate(value: &str) -> Result<(), String> {
    if value.len() > 32768 {
        return Err("Each metadata field can contain up to 32 KiB of text.".into());
    }
    if value.chars().any(|c| {
        !matches!(c, '\u{9}' | '\u{a}' | '\u{d}' | '\u{20}'..='\u{d7ff}'
            | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
    }) {
        return Err("Metadata contains a character that XMP cannot store.".into());
    }
    Ok(())
}

fn fields(draft: &EditableMetadata) -> [&str; 5] {
    [
        &draft.title,
        &draft.description,
        &draft.author,
        &draft.copyright,
        &draft.keywords,
    ]
}

fn alternate(items: impl Iterator<Item = (String, String)>, replacement: &str) -> String {
    let mut items = items.peekable();
    let mut result = String::new();
    if !replacement.is_empty() || items.peek().is_some() {
        result.push_str(&format!(
            "<rdf:li xml:lang='x-default'>{}</rdf:li>",
            escape(replacement)
        ));
    }
    for (lang, value) in items {
        result.push_str(&format!(
            "<rdf:li xml:lang='{}'>{}</rdf:li>",
            escape(&lang),
            escape(&value)
        ));
    }
    result
}

pub(super) fn update(
    packet: Option<&[u8]>,
    original: &EditableMetadata,
    updated: &EditableMetadata,
) -> Result<String, String> {
    let xml = text(packet.unwrap_or(EMPTY_PACKET))?;
    let document = parse(xml)?;
    let rdf = root(&document)?;
    let (subject, descriptions) = descriptions(rdf);
    let mut replacements = Vec::<(Range<usize>, String)>::new();
    let mut additions = String::new();
    for ((property, kind), (previous, replacement)) in FIELDS
        .into_iter()
        .zip(fields(original).into_iter().zip(fields(updated)))
    {
        if previous == replacement {
            continue;
        }
        validate(replacement)?;
        let mut original_nodes = Vec::new();
        for desc in descriptions.clone() {
            for attr in desc
                .attributes()
                .filter(|attr| attr.namespace() == Some(DC) && attr.name() == property)
            {
                replacements.push((attr.range(), String::new()));
            }
            for node in desc
                .children()
                .filter(|node| node.has_tag_name((DC, property)))
            {
                original_nodes.push(node);
                replacements.push((node.range(), String::new()));
            }
        }
        let (container, content) = match kind {
            Kind::Alternate => {
                let translations = original_nodes
                    .into_iter()
                    .flat_map(|node| list_members(node, "Alt"))
                    .filter_map(|item| {
                        let lang = item
                            .attribute((XML, "lang"))
                            .filter(|lang| *lang != "x-default")?;
                        Some((lang.to_owned(), text_value(item)))
                    });
                ("Alt", alternate(translations, replacement))
            }
            Kind::List(name) => {
                let mut entries = String::new();
                for item in replacement
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    entries.push_str(&format!("<rdf:li>{}</rdf:li>", escape(item)));
                }
                (name, entries)
            }
        };
        if !content.is_empty() {
            additions.push_str(&format!(
                "<dc:{property}><rdf:{container}>{content}</rdf:{container}></dc:{property}>"
            ));
        }
    }
    if !additions.is_empty() {
        let description = format!(
            "<rdf:Description xmlns:rdf='{RDF}' xmlns:dc='{DC}' rdf:about='{}'>{additions}</rdf:Description>",
            escape(subject),
        );
        let range = rdf.range();
        let fragment = &xml[range.clone()];
        if let Some(end) = fragment.rfind("</") {
            let position = range.start + end;
            replacements.push((position..position, description));
        } else if fragment.ends_with("/>") {
            let qname = fragment[1..]
                .split(|c: char| c.is_whitespace() || matches!(c, '/' | '>'))
                .next()
                .ok_or_else(|| "Could not locate the XMP RDF root.".to_owned())?;
            replacements.push((
                range.end - 2..range.end,
                format!(">{description}</{qname}>"),
            ));
        } else {
            return Err("Could not locate the XMP RDF root.".into());
        }
    }
    let mut result = xml.to_owned();
    replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, replacement) in replacements {
        result.replace_range(range, &replacement);
    }
    parse(&result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_xmp_fields_round_trip() {
        let fields = EditableMetadata {
            title: "Test & trial <1>".into(),
            description: "Second line\nTürkçe".into(),
            author: "Orhan, Ayşe".into(),
            copyright: "Text 'quoted'".into(),
            keywords: "photo, kitty".into(),
        };
        let packet = update(None, &EditableMetadata::default(), &fields).unwrap();
        assert_eq!(read(packet.as_bytes()).unwrap(), fields);
    }

    #[test]
    fn editing_one_field_preserves_translations_and_unrelated_elements() {
        let packet = format!(
            r#"<metadata><r:RDF xmlns:r="{RDF}"><r:Description xmlns:dc="{DC}" dc:identifier="retain" dc:rights="Keep me"><dc:title><r:Alt><r:li xml:lang="x-default">Old</r:li><r:li xml:lang="fr">Ancien</r:li></r:Alt></dc:title></r:Description></r:RDF></metadata>"#
        );
        let previous = read(packet.as_bytes()).unwrap();
        let updated = EditableMetadata {
            title: "New".into(),
            copyright: "New rights".into(),
            ..previous.clone()
        };
        let patched = update(Some(packet.as_bytes()), &previous, &updated).unwrap();
        assert_eq!(read(patched.as_bytes()).unwrap(), updated);
        assert!(patched.contains("dc:identifier=\"retain\""));
        assert!(patched.contains("Ancien"));
        assert!(!patched.contains("Keep me"));
    }
}
