# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The container images used outside a Dockerfile: the `container:` of the workflows and the clean systems of
# scripts/check-install-from-obs.sh. Nothing builds this file; it exists so that Dependabot updates their digests.
# check-install-from-obs.sh reads its images from here by stage name, and scripts/check-images.sh makes sure that every
# image:tag carries the same digest everywhere, so a workflow can't keep an old one.
FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2 AS debian-13
FROM ubuntu:26.04@sha256:065aa8626841f16057b78b027b464cd59c4fac36c204c72dcbf48d66f5fa8a96 AS ubuntu-26.04
# The newest Fedora release with packages on OBS.
FROM fedora:44@sha256:15da97ed9aa94dd3d92c3925eb89520f6c80e07ea7ee24cba848d1f8b6912b3e AS fedora
FROM opensuse/tumbleweed@sha256:dc714bafa362b1aa58a6fd2fcea9c2469255c1fb8f4433e943e44fd61195c09c AS opensuse-tumbleweed
FROM archlinux:base@sha256:d4594bb2300cba11f41c2f27923eb56f63d9988909c402f9c95517d1e362c899 AS archlinux
