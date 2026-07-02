BINARY := truenas-coolercontrol-sensors
PACKAGE := ./cmd/truenas-coolercontrol-sensors

.PHONY: build test clean

build:
	go build -trimpath -ldflags="-s -w" -o bin/$(BINARY) $(PACKAGE)

test:
	go test ./...

clean:
	rm -rf bin
