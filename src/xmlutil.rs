//! Small quick-xml helpers shared by the rbsort and cdjsafe XML passes.

use anyhow::Result;
use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::writer::Writer;

/// Read one attribute's unescaped value from a start tag.
pub(crate) fn get_attr(e: &BytesStart, name: &[u8]) -> Result<Option<String>> {
    for attr in e.attributes() {
        let attr = attr?;
        if attr.key.as_ref() == name {
            #[allow(deprecated)]
            return Ok(Some(attr.unescape_value()?.into_owned()));
        }
    }
    Ok(None)
}

/// Extract `(Name, Type, KeyType)` from a playlist NODE in a single attribute scan.
pub(crate) fn playlist_node_attrs(e: &BytesStart) -> Result<(String, String, String)> {
    let mut name = String::new();
    let mut ty = String::new();
    let mut key_type = String::new();
    for attr in e.attributes() {
        let attr = attr?;
        #[allow(deprecated)]
        let val = || -> Result<String> { Ok(attr.unescape_value()?.into_owned()) };
        match attr.key.as_ref() {
            b"Name" => name = val()?,
            b"Type" => ty = val()?,
            b"KeyType" => key_type = val()?,
            _ => {}
        }
    }
    Ok((name, ty, key_type))
}

/// Rewrite a start tag with one numeric attribute increased by `add`.
pub(crate) fn bump_count_attr(
    e: &BytesStart,
    attr_name: &[u8],
    add: usize,
) -> Result<BytesStart<'static>> {
    let name = String::from_utf8(e.name().as_ref().to_vec())?;
    let mut new_start = BytesStart::new(name);
    let mut seen = false;
    for attr in e.attributes() {
        let attr = attr?;
        if attr.key.as_ref() == attr_name {
            seen = true;
            let val: usize = std::str::from_utf8(&attr.value)?.trim().parse().unwrap_or(0);
            let new_val = (val + add).to_string();
            new_start.push_attribute((std::str::from_utf8(attr_name)?, new_val.as_str()));
        } else {
            new_start.push_attribute(attr.to_owned());
        }
    }
    if !seen {
        new_start.push_attribute((std::str::from_utf8(attr_name)?, add.to_string().as_str()));
    }
    Ok(new_start)
}

/// Emit a `<NODE Type="1" KeyType="0">` playlist holding `<TRACK Key=…/>` refs.
pub(crate) fn emit_playlist<W: std::io::Write>(
    writer: &mut Writer<W>,
    name: &str,
    track_ids: &[String],
) -> Result<()> {
    let entries = track_ids.len().to_string();
    let mut node = BytesStart::new("NODE");
    node.push_attribute(("Name", name));
    node.push_attribute(("Type", "1"));
    node.push_attribute(("KeyType", "0"));
    node.push_attribute(("Entries", entries.as_str()));
    writer.write_event(Event::Start(node))?;

    for tid in track_ids {
        let mut track = BytesStart::new("TRACK");
        track.push_attribute(("Key", tid.as_str()));
        writer.write_event(Event::Empty(track))?;
    }

    writer.write_event(Event::End(BytesEnd::new("NODE")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::events::BytesStart;

    fn start_with_attr(tag: &str, key: &str, val: &str) -> BytesStart<'static> {
        let mut e = BytesStart::new(tag.to_owned());
        e.push_attribute((key, val));
        e
    }

    #[test]
    fn get_attr_returns_value() {
        let e = start_with_attr("COLLECTION", "Entries", "5");
        assert_eq!(get_attr(&e, b"Entries").unwrap(), Some("5".to_string()));
    }

    #[test]
    fn get_attr_missing_returns_none() {
        let e = start_with_attr("COLLECTION", "Entries", "5");
        assert_eq!(get_attr(&e, b"Missing").unwrap(), None);
    }

    #[test]
    fn bump_count_attr_increments_existing() {
        let e = start_with_attr("COLLECTION", "Entries", "3");
        let bumped = bump_count_attr(&e, b"Entries", 2).unwrap();
        assert_eq!(get_attr(&bumped, b"Entries").unwrap(), Some("5".to_string()));
    }

    #[test]
    fn bump_count_attr_adds_missing_attr() {
        let e = BytesStart::new("COLLECTION");
        let bumped = bump_count_attr(&e, b"Entries", 1).unwrap();
        assert_eq!(get_attr(&bumped, b"Entries").unwrap(), Some("1".to_string()));
    }

    #[test]
    fn bump_count_attr_preserves_other_attrs() {
        let mut e = BytesStart::new("NODE");
        e.push_attribute(("Name", "MyList"));
        e.push_attribute(("Entries", "2"));
        let bumped = bump_count_attr(&e, b"Entries", 1).unwrap();
        assert_eq!(get_attr(&bumped, b"Name").unwrap(), Some("MyList".to_string()));
        assert_eq!(get_attr(&bumped, b"Entries").unwrap(), Some("3".to_string()));
    }
}
