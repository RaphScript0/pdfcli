fn main() -> anyhow::Result<()> {
    let mut doc = lopdf::Document::with_version("1.4");

    let pages_tree_id = doc.new_object_id();
    let leaf_page_id = doc.new_object_id();
    let catalog_obj_id = doc.new_object_id();

    doc.objects.insert(
        pages_tree_id,
        lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
            (b"Type".to_vec(), lopdf::Object::Name(b"Pages".to_vec())),
            (
                b"Kids".to_vec(),
                lopdf::Object::Array(vec![lopdf::Object::Reference(leaf_page_id)]),
            ),
            (b"Count".to_vec(), lopdf::Object::Integer(1)),
        ])),
    );

    doc.objects.insert(
        leaf_page_id,
        lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
            (b"Type".to_vec(), lopdf::Object::Name(b"Page".to_vec())),
            (b"Parent".to_vec(), lopdf::Object::Reference(pages_tree_id)),
            (
                b"MediaBox".to_vec(),
                lopdf::Object::Array(vec![
                    lopdf::Object::Integer(0),
                    lopdf::Object::Integer(0),
                    lopdf::Object::Integer(612),
                    lopdf::Object::Integer(792),
                ]),
            ),
        ])),
    );

    doc.objects.insert(
        catalog_obj_id,
        lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
            (b"Type".to_vec(), lopdf::Object::Name(b"Catalog".to_vec())),
            (b"Pages".to_vec(), lopdf::Object::Reference(pages_tree_id)),
        ])),
    );

    doc.trailer
        .set(b"Root", lopdf::Object::Reference(catalog_obj_id));

    doc.save("sample.pdf")?;
    eprintln!("wrote sample.pdf");

    Ok(())
}
