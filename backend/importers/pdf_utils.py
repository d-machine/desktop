"""PDF open helper — tries pdfplumber directly, falls back to pypdf decrypt → temp file."""
import tempfile
from pathlib import Path


def open_pdf(file_path: str, password: str | None = None):
    """
    Return a pdfplumber PDF object.
    If the PDF is encrypted and a password is provided, decrypt via pypdf to a
    temporary file first, then open with pdfplumber.
    Caller is responsible for calling .close() on the returned object.
    """
    import pdfplumber

    try:
        pdf = pdfplumber.open(file_path, password=password)
        _ = pdf.pages[0]  # trigger read to verify it's accessible
        return pdf
    except Exception as first_err:
        if password is None:
            raise

    # Encrypted — decrypt to a temp file with pypdf
    try:
        import pypdf

        reader = pypdf.PdfReader(file_path)
        result = reader.decrypt(password)
        if result == 0:
            raise ValueError(f"Wrong PDF password for {file_path}")

        writer = pypdf.PdfWriter()
        for page in reader.pages:
            writer.add_page(page)

        tmp = tempfile.NamedTemporaryFile(suffix=".pdf", delete=False)
        writer.write(tmp)
        tmp.close()

        return pdfplumber.open(tmp.name)
    except Exception as e:
        raise ValueError(f"Could not open PDF {file_path}: {e}") from first_err


def extract_text_lines(file_path: str, password: str | None = None) -> list[str]:
    """Extract all text from a PDF as a flat list of non-empty lines."""
    with open_pdf(file_path, password) as pdf:
        lines = []
        for page in pdf.pages:
            text = page.extract_text() or ""
            lines.extend(line for line in text.splitlines() if line.strip())
    return lines
