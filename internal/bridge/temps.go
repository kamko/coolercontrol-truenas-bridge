package bridge

import (
	"encoding/json"
	"math"
	"regexp"
	"strconv"
	"strings"
	"time"
)

var unsafeSensorName = regexp.MustCompile(`[^A-Za-z0-9_.-]+`)

type TemperatureSample struct {
	TemperaturesC map[string]float64 `json:"temperatures_c"`
	SourceOK      bool               `json:"source_ok"`
	MeasuredAt    time.Time          `json:"measured_at"`
	Error         string             `json:"error,omitempty"`
}

func (s TemperatureSample) HDDMaxC() float64 {
	maximum := math.NaN()
	for _, value := range s.TemperaturesC {
		if math.IsNaN(maximum) || value > maximum {
			maximum = value
		}
	}
	return maximum
}

func (s TemperatureSample) JSON() string {
	data, err := json.MarshalIndent(s, "", "  ")
	if err != nil {
		return "{}"
	}
	return string(data)
}

func NormalizeTemperatures(raw map[string]any) map[string]float64 {
	temperatures := make(map[string]float64)
	for diskName, value := range raw {
		temp, ok := ExtractTemperatureC(value)
		if !ok {
			continue
		}
		temperatures[diskName] = temp
	}
	return temperatures
}

func ExtractTemperatureC(value any) (float64, bool) {
	switch typed := value.(type) {
	case float64:
		return typed, true
	case int:
		return float64(typed), true
	case map[string]any:
		for _, key := range []string{"temperature", "temp", "value", "current"} {
			if nested, ok := typed[key]; ok {
				if temp, ok := ExtractTemperatureC(nested); ok {
					return temp, true
				}
			}
		}
	}
	return 0, false
}

func MillicelsiusText(tempC float64) string {
	return strconv.FormatInt(int64(math.Round(tempC*1000)), 10) + "\n"
}

func SafeSensorName(name string) string {
	safe := unsafeSensorName.ReplaceAllString(name, "_")
	safe = strings.Trim(safe, "._-")
	if safe == "" {
		return "disk"
	}
	return safe
}
