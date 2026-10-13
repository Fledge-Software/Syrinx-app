"""Serve fresh untrusted loopback TLS using installed standard libraries only."""

import base64
import json
import os
from pathlib import Path
import socket
import ssl
import subprocess
import time


def der(tag, value):
    """Encode the small ASN.1 values needed for a synthetic PKCS#1 private key."""
    length = len(value)
    size = length.to_bytes(max(1, (length.bit_length() + 7) // 8), "big")
    prefix = bytes([length]) if length < 128 else bytes([0x80 | len(size)]) + size
    return bytes([tag]) + prefix + value


def integer(value):
    value = value.lstrip(b"\0") or b"\0"
    return der(2, b"\0" + value if value[0] & 0x80 else value)


def pem(label, value):
    body = base64.encodebytes(value).decode("ascii")
    return f"-----BEGIN {label}-----\n{body}-----END {label}-----\n"


def certificate_context(directory):
    script = Path(os.environ["ATLAS_SYNTHETIC_CERT_GENERATOR"]).read_text(encoding="utf-8")
    generated = subprocess.run(
        ["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", script],
        capture_output=True, timeout=15, creationflags=subprocess.CREATE_NO_WINDOW,
    )
    if generated.returncode:
        raise RuntimeError("Synthetic certificate generation failed; no key material is logged.")
    values = json.loads(generated.stdout)
    key = der(0x30, integer(b"\0") + b"".join(
        integer(base64.b64decode(values[name]))
        for name in ("modulus", "exponent", "d", "p", "q", "dp", "dq", "iq")
    ))
    certificate = directory / "synthetic-certificate.pem"
    private_key = directory / "synthetic-key.pem"
    # The untrusted, freshly generated test key stays in this exclusive synthetic directory.
    with certificate.open("x", encoding="ascii") as output:
        output.write(pem("CERTIFICATE", base64.b64decode(values["certificate"])))
    with private_key.open("x", encoding="ascii") as output:
        output.write(pem("RSA PRIVATE KEY", key))
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(certificate, private_key)
    return context


def serve():
    directory = Path(os.environ["ATLAS_SYNTHETIC_CERT_DIRECTORY"])
    context = certificate_context(directory)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        listener.settimeout(0.2)
        with (directory / "port").open("x", encoding="ascii") as output:
            output.write(str(listener.getsockname()[1]))
        deadline = time.monotonic() + 75
        while time.monotonic() < deadline:
            try:
                client, _ = listener.accept()
            except TimeoutError:
                continue
            with client:
                client.setblocking(True)
                client.settimeout(3)
                try:
                    with context.wrap_socket(client, server_side=True) as stream:
                        if not stream.recv(1):
                            continue
                        with (directory / "delivered").open("a", encoding="ascii") as output:
                            output.write("request\n")
                        body = b"<!doctype html><title>Invalid certificate content reached</title><link rel=icon href=data:,>This response must never reach the page."
                        stream.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: " + str(len(body)).encode("ascii") + b"\r\nConnection: close\r\n\r\n" + body)
                except (ssl.SSLError, ConnectionError, TimeoutError):
                    # Certificate rejection and abandoned speculative sockets are expected.
                    # The native fixture independently requires the certificate-error event.
                    print("Synthetic TLS connection ended without a completed response.", flush=True)


serve()
