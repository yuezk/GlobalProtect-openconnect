#!/bin/bash -e

git describe --tags --match "v$(cat VERSION)" | sed -r -e 's/v([^-]+)-/+snapshot/' -e 's/-/./' > VERSION_SUFFIX
