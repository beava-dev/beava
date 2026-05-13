FROM python:3.12-slim
WORKDIR /app

# The beava Python SDK is on PyPI; pin a version that matches the server.
# The wheel ships the Rust client transport (httpx-based), no native deps.
RUN pip install --no-cache-dir 'beava>=0.0.3'

COPY generator.py /app/generator.py

EXPOSE 8090
CMD ["python", "-u", "generator.py"]
