# Makefile for building the tock kernel for the CC2650-CherryMote

TOCK_BOARD=ti-cc2650-cherry_mote
TARGET=thumbv7m-none-eabi
PLATFORM=ti-cc2650-cherry-mote

include ../../Makefile.common
include ../common/Makefile.common

HENI_DEVICE ?= 677
HENI_PROGRAM := $(HENI) node program cherry -d dev:$(HENI_DEVICE)
HENI_SCP := $(HENI) supervisor scp -d dev:$(HENI_DEVICE)

### Flashing kernel only
.PHONY: flash
flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).flash
	$(HENI_PROGRAM) $<

### Flashing kernel + an app
.PHONY: app-*-flash
app-%-flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-%.flash
	$(HENI_PROGRAM) $<

### SCP kernel + an app .bin
.PHONY: app-*-bin-scp
app-%-bin-scp: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-%.bin
	$(HENI_SCP) $< /tmp/node_program.bin
