use super::*;
use skrifa::{raw::TableProvider, MetadataProvider};

#[test]
fn simple_instance_retains_cmap_and_every_glyph_id() {
    let data = include_bytes!("../fonts/NotoSans-Regular_var.ttf");
    let source = skrifa::FontRef::new(data).unwrap();
    let result = instance_for_simple_pdf(data, 0, &[(Tag::new(b"wght"), 700.0)]).unwrap();
    let instance = skrifa::FontRef::new(&result).unwrap();
    assert_eq!(
        source.maxp().unwrap().num_glyphs(),
        instance.maxp().unwrap().num_glyphs()
    );
    assert!(instance.cmap().is_ok(), "simple PDF fonts require cmap");
    assert_eq!(
        source.charmap().mappings().collect::<Vec<_>>(),
        instance.charmap().mappings().collect::<Vec<_>>()
    );
    let location = source.axes().location([("wght", 700.0)]);
    let expected = source.glyph_metrics(skrifa::instance::Size::unscaled(), &location);
    let actual = instance.glyph_metrics(
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::default(),
    );
    for id in 0..source.maxp().unwrap().num_glyphs() {
        let id = skrifa::GlyphId::new(u32::from(id));
        assert_eq!(
            actual.advance_width(id).unwrap(),
            expected.advance_width(id).unwrap().round()
        );
    }
    assert_eq!(checksum(&result), 0xB1B0AFBA);
    let other = instance_for_simple_pdf(data, 0, &[(Tag::new(b"wght"), 900.0)]).unwrap();
    assert_ne!(result, other);
    for tag in [b"fvar", b"gvar", b"HVAR", b"MVAR"] {
        assert!(instance.table_data(skrifa::Tag::new(tag)).is_none());
    }
}

#[test]
fn simple_instance_extracts_the_selected_collection_face() {
    let data = include_bytes!("../fonts/TestTTC.ttc");
    let face = parse(data, 1).unwrap();
    let index = face
        .records
        .iter()
        .position(|record| record.tag == Tag::HMTX)
        .unwrap();
    let mut metrics = face.table(Tag::HMTX).unwrap().to_vec();
    metrics[..2].copy_from_slice(&777u16.to_be_bytes());
    let mut collection = data.to_vec();
    let table_offset = collection.len() as u32;
    collection.extend(metrics);
    let face_offset = u32::from_be_bytes(data[16..20].try_into().unwrap()) as usize;
    let record_offset = face_offset + 12 + index * 16 + 8;
    collection[record_offset..record_offset + 4]
        .copy_from_slice(&table_offset.to_be_bytes());
    let result = instance_for_simple_pdf(&collection, 1, &[]).unwrap();
    assert_ne!(&result[..4], b"ttcf");
    let source = skrifa::FontRef::from_index(&collection, 1).unwrap();
    let instance = skrifa::FontRef::new(&result).unwrap();
    assert!(instance.cmap().is_ok(), "selected face must keep its cmap");
    let advance = |font: &skrifa::FontRef<'_>| {
        font.glyph_metrics(
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::default(),
        )
        .advance_width(skrifa::GlyphId::new(0))
        .unwrap()
    };
    assert_eq!(advance(&source), 777.0);
    assert_ne!(advance(&skrifa::FontRef::from_index(&collection, 0).unwrap()), 777.0);
    assert_eq!(advance(&instance), 777.0);

    assert_eq!(
        source.charmap().mappings().collect::<Vec<_>>(),
        instance.charmap().mappings().collect::<Vec<_>>()
    );
}

fn overwrite(data: &[u8], tag: &[u8; 4], relative: usize, bytes: &[u8]) -> Vec<u8> {
    let face = parse(data, 0).unwrap();
    let offset = face
        .records
        .iter()
        .find(|record| record.tag == Tag::new(tag))
        .unwrap()
        .offset as usize
        + relative;
    let mut result = data.to_vec();
    result[offset..offset + bytes.len()].copy_from_slice(bytes);
    result
}

#[test]
fn simple_instance_rejects_invalid_faces_and_required_tables() {
    let data = include_bytes!("../fonts/NotoSans-Regular.ttf");
    let collection = include_bytes!("../fonts/TestTTC.ttc");
    for (bytes, index) in [
        (data.as_slice(), 1),
        (collection.as_slice(), 2),
        (collection.as_slice(), u32::MAX),
    ] {
        assert!(instance_for_simple_pdf(bytes, index, &[]).is_err());
    }
    for (tag, offset, value) in [
        (b"maxp", 4, 0u16),
        (b"head", 18, 0),
        (b"hhea", 34, 0),
        (b"hhea", 34, u16::MAX),
        (b"head", 50, 2),
    ] {
        assert!(instance_for_simple_pdf(
            &overwrite(data, tag, offset, &value.to_be_bytes()),
            0,
            &[]
        )
        .is_err());
    }
    let face = parse(data, 0).unwrap();
    for tag in [
        Tag::HEAD,
        Tag::HHEA,
        Tag::HMTX,
        Tag::MAXP,
        Tag::LOCA,
        Tag::GLYF,
        Tag::CMAP,
        Tag::NAME,
        Tag::POST,
    ] {
        let index = face.records.iter().position(|record| record.tag == tag).unwrap();
        let mut missing = data.to_vec();
        missing[12 + index * 16..16 + index * 16].copy_from_slice(b"BAD!");
        assert!(instance_for_simple_pdf(&missing, 0, &[]).is_err(), "missing {tag:?}");
        let mut truncated = data.to_vec();
        truncated[24 + index * 16..28 + index * 16].copy_from_slice(&0u32.to_be_bytes());
        assert!(
            instance_for_simple_pdf(&truncated, 0, &[]).is_err(),
            "truncated {tag:?}"
        );
    }
}

#[test]
fn simple_instance_rejects_invalid_cmap_targets_and_components() {
    let data = include_bytes!("../fonts/NotoSans-Regular.ttf");
    let font = skrifa::FontRef::new(data).unwrap();
    let cmap = font.cmap().unwrap();
    let record = cmap
        .encoding_records()
        .iter()
        .find(|record| record.platform_id() as u16 == 3 && record.encoding_id() == 1)
        .unwrap();
    let table = record.subtable(cmap.offset_data()).unwrap();
    let skrifa::raw::tables::cmap::CmapSubtable::Format4(table) = table else {
        panic!("fixture has format4 cmap")
    };
    let delta_offset = record.subtable_offset().to_u32() as usize
        + 16
        + table.seg_count_x2() as usize * 2;
    let invalid_cmap = overwrite(data, b"cmap", delta_offset, &30000i16.to_be_bytes());
    assert!(instance_for_simple_pdf(&invalid_cmap, 0, &[]).is_err());
    let loca = font.loca(None).unwrap();
    let glyf = font.glyf().unwrap();
    let id = (0..font.maxp().unwrap().num_glyphs())
        .find(|id| {
            loca.get_glyf(skrifa::GlyphId::new(u32::from(*id)), &glyf)
                .unwrap()
                .is_some_and(|glyph| glyph.number_of_contours() < 0)
        })
        .unwrap();
    let component_offset = loca.get_raw(id as usize).unwrap() as usize + 12;
    let invalid_component =
        overwrite(data, b"glyf", component_offset, &u16::MAX.to_be_bytes());
    assert!(instance_for_simple_pdf(&invalid_component, 0, &[]).is_err());
}

#[test]
fn simple_instance_bounds_enclose_the_emitted_glyph_headers() {
    let data = include_bytes!("../fonts/NotoSans-Regular_var.ttf");
    let narrow_head = overwrite(data, b"head", 36, &[0; 8]);
    let result =
        instance_for_simple_pdf(&narrow_head, 0, &[(Tag::new(b"wght"), 900.0)]).unwrap();
    let font = skrifa::FontRef::new(&result).unwrap();
    let head = font.head().unwrap();
    assert!(head.x_max() > 0);
    let loca = font.loca(None).unwrap();
    let glyf = font.glyf().unwrap();
    for id in 0..font.maxp().unwrap().num_glyphs() {
        if let Some(glyph) =
            loca.get_glyf(skrifa::GlyphId::new(u32::from(id)), &glyf).unwrap()
        {
            assert!(head.x_min() <= glyph.x_min() && head.y_min() <= glyph.y_min());
            assert!(head.x_max() >= glyph.x_max() && head.y_max() >= glyph.y_max());
        }
    }
}

#[test]
fn ordinary_cid_subsets_still_omit_cmap() {
    let result =
        subset(include_bytes!("../fonts/NotoSans-Regular.ttf"), 0, &GlyphRemapper::new())
            .unwrap();
    assert!(skrifa::FontRef::new(&result).unwrap().cmap().is_err());
}
