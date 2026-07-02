package bridge

import "testing"

func TestNormalizeTemperatures(t *testing.T) {
	got := NormalizeTemperatures(map[string]any{
		"sda": float64(41),
		"sdb": map[string]any{"temperature": float64(42.5), "threshold": float64(60)},
		"sdc": nil,
	})

	if got["sda"] != 41 {
		t.Fatalf("sda = %v, want 41", got["sda"])
	}
	if got["sdb"] != 42.5 {
		t.Fatalf("sdb = %v, want 42.5", got["sdb"])
	}
	if _, ok := got["sdc"]; ok {
		t.Fatalf("sdc should be omitted")
	}
}

func TestMillicelsiusText(t *testing.T) {
	if got := MillicelsiusText(42.49); got != "42490\n" {
		t.Fatalf("MillicelsiusText = %q, want 42490\\n", got)
	}
}

func TestSafeSensorName(t *testing.T) {
	if got := SafeSensorName("disk bay/1"); got != "disk_bay_1" {
		t.Fatalf("SafeSensorName = %q, want disk_bay_1", got)
	}
}
