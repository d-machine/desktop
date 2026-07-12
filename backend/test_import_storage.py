from pathlib import Path

from routers import import_ as import_router


def test_save_import_document_copies_to_storage_dir(tmp_path: Path) -> None:
    source_path = tmp_path / "sample-contract-note.pdf"
    source_path.write_bytes(b"contract-note-content")

    storage_dir = tmp_path / "imported_documents"
    saved_path = import_router._save_import_document(storage_dir, str(source_path), "BAJAJ_FINANCE")

    saved = Path(saved_path)
    assert saved.exists()
    assert saved.parent == storage_dir
    assert saved.read_bytes() == b"contract-note-content"
    assert saved.name.startswith("sample-contract-note")
