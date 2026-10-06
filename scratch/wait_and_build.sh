#!/bin/bash
echo "Waiting for docker build to finish..."
while docker ps | grep "ruview-appimage-copy" > /dev/null; do
    sleep 5
done
# Actually, the docker build is running as task 301. We can just wait for the tag ruview-local:latest.
# But wait, we can just chain it. The docker build is running via the API, so I'll wait until docker image inspect works.
# Wait, docker image inspect works BEFORE it finishes if it existed previously.
# I'll just write a loop that checks if cargo build is running inside docker, but we can't easily do that.
