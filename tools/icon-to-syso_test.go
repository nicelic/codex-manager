package main

import (
	"encoding/binary"
	"testing"
)

func TestBuildResourceObjectUsesValidNumericDirectoryEntries(t *testing.T) {
	object, err := buildResourceObject([]icoImage{{width: 16, height: 16, planes: 1, bitCount: 32, data: []byte{1, 2, 3, 4}}})
	if err != nil {
		t.Fatalf("buildResourceObject() error = %v", err)
	}
	const section1Offset = coffFileHeaderSize + 2*coffSectionHeaderSize
	if len(object) < section1Offset+16+16 {
		t.Fatalf("resource object too small: %d", len(object))
	}
	directory := object[section1Offset:]
	if got := binary.LittleEndian.Uint16(directory[12:14]); got != 0 {
		t.Fatalf("root named entry count = %d, want 0", got)
	}
	if got := binary.LittleEndian.Uint16(directory[14:16]); got != 2 {
		t.Fatalf("root numeric entry count = %d, want 2", got)
	}
	firstEntry := directory[16:24]
	secondEntry := directory[24:32]
	if binary.LittleEndian.Uint32(firstEntry[4:8])&0x80000000 == 0 {
		t.Fatal("first resource type entry is not a subdirectory")
	}
	if binary.LittleEndian.Uint32(secondEntry[4:8])&0x80000000 == 0 {
		t.Fatal("second resource type entry is not a subdirectory")
	}
	if binary.LittleEndian.Uint32(firstEntry[0:4]) != imageResourceTypeIcon || binary.LittleEndian.Uint32(secondEntry[0:4]) != imageResourceTypeGroupIcon {
		t.Fatalf("resource type order = %d, %d; want %d, %d", binary.LittleEndian.Uint32(firstEntry[0:4]), binary.LittleEndian.Uint32(secondEntry[0:4]), imageResourceTypeIcon, imageResourceTypeGroupIcon)
	}
	resourceDataSize := func(typeEntry []byte) uint32 {
		typeDirectoryOffset := int(binary.LittleEndian.Uint32(typeEntry[4:8]) &^ 0x80000000)
		languageEntry := directory[typeDirectoryOffset+16 : typeDirectoryOffset+24]
		languageDirectoryOffset := int(binary.LittleEndian.Uint32(languageEntry[4:8]) &^ 0x80000000)
		dataEntryOffset := int(binary.LittleEndian.Uint32(directory[languageDirectoryOffset+20:languageDirectoryOffset+24]) &^ 0x80000000)
		return binary.LittleEndian.Uint32(directory[dataEntryOffset+4 : dataEntryOffset+8])
	}
	if got := resourceDataSize(firstEntry); got != 4 {
		t.Fatalf("RT_ICON data size = %d, want 4", got)
	}
	if got := resourceDataSize(secondEntry); got != 20 {
		t.Fatalf("RT_GROUP_ICON data size = %d, want 20", got)
	}
}
