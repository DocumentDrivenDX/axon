# Axon quickstart demo recording image
#
# Builds on top of axon:demo (built from the repo root Dockerfile) and adds
# asciinema for terminal session capture.
#
# Build:
#   docker build -t axon:demo .                       # from repo root (main image)
#   docker build -t axon:recording docs/demos/quickstart/
#
# Record:
#   mkdir -p website/static/demos
#   docker run --rm \
#     -e TERM=xterm-256color \
#     -v $(pwd)/website/static/demos:/recordings \
#     axon:recording
#
# The cast file is saved to website/static/demos/quickstart.cast.

FROM axon:demo

USER root

RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 python3-pip \
    && pip3 install asciinema --break-system-packages \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /recordings /root && chmod 777 /recordings

ENV HOME=/root
ENV TERM=xterm-256color

ENTRYPOINT ["asciinema", "rec", \
    "--cols", "100", \
    "--rows", "35", \
    "--overwrite", \
    "/recordings/quickstart.cast", \
    "--command", "bash /scripts/demo.sh"]
