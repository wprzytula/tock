# Makefile for building the tock kernel for the CC2650-SmartRF06

# Provide your path to cc26x0 driverlib sources:
export DRIVERLIB_PATH := /home/xps15/Studia/Sem8/Tock/driverlib/cc26x0/driverlib

TOCK_BOARD=ti-cc2650-smartrf06
TARGET=thumbv7m-none-eabi
PLATFORM=ti-cc2650-smartrf06
OPENOCD ?= openocd

include ../../Makefile.common
include ../common/Makefile.common

HENI_PROGRAM := $(HENI) node program dk

### Flashing kernel only
.PHONY: flash
flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).bin
	$(OPENOCD) -f flash-kernel.openocd

### Flashing kernel + an app
.PHONY: app-%-flash
app-%-flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-%.hex
	$(HENI_PROGRAM) $<
