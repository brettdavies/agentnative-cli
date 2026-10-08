# Runtimes and synthetic framework probes for the `probes` captures in
# ../index.json: Go, Python 3.12 and 3.13, Node, Ruby and a JRE, with one
# `probe-<framework>` command per probe on PATH.
#
#   docker build --provenance=false --tag anc-help-probes tests/fixtures/help/probes
#   docker image inspect --format '{{.Id}}' anc-help-probes
#
# The second command prints the ID the index records as `images.probes.id`.
# A provenance attestation records the build time and is part of the ID, so
# without `--provenance=false` every rebuild gets a new one.

FROM golang:1.27.1-trixie@sha256:8f58fd67ea075142d947a60e0caa4317746a55118d312f027793d382c7741734 AS go-build
WORKDIR /src
COPY go/ ./
RUN for probe in stdflag kingpin kong urfave2 urfave3; do \
      CGO_ENABLED=0 go build -trimpath -o "/out/$probe" "./$probe" || exit 1; \
    done

FROM eclipse-temurin:21.0.12.1_1-jdk@sha256:3e3c176ffed168beb42c607be9bc1639b466cf00261a0fb04425562c9d0c5c2b AS java-build
ADD --chmod=644 --checksum=sha256:f86e30fffd10d2b13b8caa8d4b237a7ee61f2ffccf5b1941de718b765d235bf8 \
    https://repo1.maven.org/maven2/info/picocli/picocli/4.7.7/picocli-4.7.7.jar /out/picocli-4.7.7.jar
COPY java/Probe.java /src/Probe.java
RUN javac -cp /out/picocli-4.7.7.jar -d /out/classes /src/Probe.java

FROM eclipse-temurin:21.0.12.1_1-jre@sha256:cff19e6215689161eb6162c11b86b0c60ddf802164f2eaf48d570f8fb79a36c5 AS jre

FROM node:26.10.0-trixie-slim@sha256:930557a230abacbc3f4fd9b8648abf8f4bee1e17cb72195dcdfb2f709bc85b33 AS node-build
WORKDIR /opt/probes/node
COPY node/package.json node/package-lock.json ./
RUN npm ci --ignore-scripts
COPY node/ ./
# node links libatomic, which the Ruby base image lacks.
RUN cp -L "$(ldconfig -p | awk '/libatomic\.so\.1 / { print $NF; exit }')" /opt/libatomic.so.1

FROM python:3.12.15-slim-trixie@sha256:05cda9777409a9c3ffddd94a4c476b79f0769a0b4857f0c7ed9226b6800b0d6f AS python312

FROM python:3.13.13-slim-trixie@sha256:aa938a849bcb82dce8f49480f056ab82bf5c1c3ebc294f0430f37b6820e7f286 AS python313
COPY python/requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir --require-hashes --requirement /tmp/requirements.txt

FROM ruby:4.0.7-slim-trixie@sha256:073f6464ca7c0b66fa86715ec57339a39da08fdc803583140edc66e486af9aeb
RUN gem install thor --version 1.5.0 --no-document

COPY --from=python312 /usr/local/ /usr/local/
COPY --from=python313 /usr/local/ /usr/local/
COPY --from=node-build /usr/local/bin/node /usr/local/bin/node
COPY --from=node-build /opt/libatomic.so.1 /usr/local/lib/libatomic.so.1
COPY --from=jre /opt/java/openjdk /opt/java/openjdk
RUN ldconfig

COPY --from=go-build /out/stdflag /opt/probes/bin/probe-go-flag
COPY --from=go-build /out/kingpin /opt/probes/bin/probe-kingpin
COPY --from=go-build /out/kong /opt/probes/bin/probe-kong
COPY --from=go-build /out/urfave2 /opt/probes/bin/probe-urfave-v2
COPY --from=go-build /out/urfave3 /opt/probes/bin/probe-urfave-v3
COPY --from=java-build /out/ /opt/probes/java/
COPY --from=node-build /opt/probes/node/ /opt/probes/node/
COPY python/*.py /opt/probes/python/
COPY ruby/thor-probe.rb /opt/probes/ruby/thor-probe.rb

RUN wrap() { \
      printf '#!/bin/sh\nexec %s "$@"\n' "$2" >"/opt/probes/bin/$1" && chmod +x "/opt/probes/bin/$1"; \
    } \
    && wrap probe-argparse-py3.12 'python3.12 /opt/probes/python/py_argparse.py' \
    && wrap probe-argparse-py3.13 'python3.13 /opt/probes/python/py_argparse.py' \
    && wrap probe-click 'python3.13 /opt/probes/python/py_click.py' \
    && wrap probe-docopt 'python3.13 /opt/probes/python/py_docopt.py' \
    && wrap probe-rich-click 'python3.13 /opt/probes/python/py_richclick.py' \
    && wrap probe-typer 'python3.13 /opt/probes/python/py_typer.py' \
    && wrap probe-commander 'node /opt/probes/node/commander.js' \
    && wrap probe-yargs 'node /opt/probes/node/yargs.js' \
    && wrap probe-oclif 'node /opt/probes/node/oclif-run.mjs' \
    && wrap probe-thor 'ruby /opt/probes/ruby/thor-probe.rb' \
    && wrap probe-picocli 'java -cp /opt/probes/java/picocli-4.7.7.jar:/opt/probes/java/classes Probe'

ENV PATH=/opt/probes/bin:/opt/java/openjdk/bin:$PATH \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8

RUN useradd --create-home runner
USER runner
WORKDIR /home/runner
