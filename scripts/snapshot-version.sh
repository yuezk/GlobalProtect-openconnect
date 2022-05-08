#!/bin/bash -e

git describe --tags --match "v$(cat VERSION)" | sed -r -e 's/-/+snapshot/' -e 's/^v//' > VERSION
