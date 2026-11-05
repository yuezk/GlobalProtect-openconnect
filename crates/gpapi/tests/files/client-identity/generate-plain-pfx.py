"""Assemble public, unencrypted PKCS#12 fixtures; no cryptography is performed."""
import base64
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def pem(name):
    lines = (ROOT / name).read_text().splitlines()
    return base64.b64decode("".join(line for line in lines if not line.startswith("-----")))


def tlv(tag, content):
    length = len(content)
    encoded = length.to_bytes(max(1, (length.bit_length() + 7) // 8), "big")
    prefix = bytes([length]) if length < 128 else bytes([128 + len(encoded)]) + encoded
    return bytes([tag]) + prefix + content


def sequence(*items):
    return tlv(0x30, b"".join(items))


def oid(value):
    return tlv(6, bytes.fromhex(value))


def explicit(content):
    return tlv(0xA0, content)


def data(content):
    return sequence(oid("2a864886f70d010701"), explicit(tlv(4, content)))


def key(name):
    return sequence(oid("2a864886f70d010c0a0101"), explicit(pem(name)))


def certificate(name):
    cert_bag = sequence(oid("2a864886f70d01091601"), explicit(tlv(4, pem(name))))
    return sequence(oid("2a864886f70d010c0a0103"), explicit(cert_bag))


def archive(name, *bags):
    (ROOT / name).write_bytes(sequence(tlv(2, b"\x03"), data(sequence(data(sequence(*bags))))))


archive("no-bag-ids.p12", certificate("root.pem"), certificate("ec-cert.pem"),
        key("rsa-pkcs8.pem"), certificate("rsa-cert.pem"))
archive("mismatched.p12", key("rsa-pkcs8.pem"), certificate("ec-cert.pem"), certificate("root.pem"))
archive("multiple-identities.p12", key("rsa-pkcs8.pem"), key("ec-pkcs8.pem"),
        certificate("rsa-cert.pem"), certificate("ec-cert.pem"))
