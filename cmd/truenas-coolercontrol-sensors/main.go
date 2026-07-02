package main

import (
	"context"
	"flag"
	"fmt"
	"log"
	"os"
	"os/signal"
	"syscall"
	"time"

	"truenas-coolercontrol-sensors/internal/bridge"
)

func main() {
	configPath := flag.String("config", "/etc/truenas-coolercontrol-sensors.json", "path to JSON config file")
	once := flag.Bool("once", false, "poll once and exit")
	printSample := flag.Bool("print-sample", false, "print parsed sample JSON after polling")
	flag.Parse()

	config, err := bridge.LoadConfig(*configPath)
	if err != nil {
		log.Fatalf("load config: %v", err)
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	service := bridge.New(config)

	if *once {
		sample, err := service.PollAndWrite(ctx)
		if err != nil {
			log.Fatalf("poll: %v", err)
		}
		if *printSample {
			fmt.Println(sample.JSON())
		}
		return
	}

	ticker := time.NewTicker(config.Polling.PollInterval)
	defer ticker.Stop()

	for {
		if _, err := service.PollAndWrite(ctx); err != nil {
			log.Printf("polling cycle failed: %v", err)
		}

		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}
