package bridge

import (
	"context"
	"fmt"
	"log"
	"time"
)

type Service struct {
	config     Config
	lastGood   *TemperatureSample
	lastGoodAt time.Time
}

func New(config Config) *Service {
	return &Service{config: config}
}

func (s *Service) PollAndWrite(ctx context.Context) (TemperatureSample, error) {
	sample := s.fetchSample(ctx)
	if err := WriteSample(s.config.Export, sample, s.config.Polling.FailsafeTemperature); err != nil {
		return sample, err
	}

	log.Printf(
		"wrote %d disk temperatures; hdd_max=%.1f C; source_ok=%v",
		len(sample.TemperaturesC),
		sample.HDDMaxC(),
		sample.SourceOK,
	)
	return sample, nil
}

func (s *Service) fetchSample(ctx context.Context) TemperatureSample {
	client := NewTrueNASClient(s.config.TrueNAS, s.config.Polling.ConnectTimeout)

	temperatures, err := client.DiskTemperatures(ctx)
	now := time.Now()
	if err == nil {
		sample := TemperatureSample{
			TemperaturesC: temperatures,
			SourceOK:      true,
			MeasuredAt:    now,
		}
		s.lastGood = &sample
		s.lastGoodAt = now
		return sample
	}

	log.Printf("failed to fetch TrueNAS temperatures: %v", err)
	if s.lastGood != nil && now.Sub(s.lastGoodAt) <= s.config.Polling.StaleAfter {
		return TemperatureSample{
			TemperaturesC: s.lastGood.TemperaturesC,
			SourceOK:      false,
			MeasuredAt:    s.lastGood.MeasuredAt,
			Error:         err.Error(),
		}
	}

	return TemperatureSample{
		TemperaturesC: map[string]float64{
			"failsafe": s.config.Polling.FailsafeTemperature,
		},
		SourceOK:   false,
		MeasuredAt: now,
		Error:      fmt.Sprintf("using failsafe temperature after fetch failure: %v", err),
	}
}
