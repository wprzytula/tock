# Makefile for building the tock kernel for the CC2650-SmartRF06

TOCK_BOARD=ti-cc2650-smartrf06
TARGET=thumbv7m-none-eabi
PLATFORM=ti-cc2650-smartrf06
OPENOCD ?= openocd

include ../../Makefile.common

LIBTOCK_RS_TARGET := $(TOCK_ROOT_DIRECTORY)../libtock-rs/target/cc2650dk/thumbv7m-none-eabi/release/examples
HENI := docker run --privileged --rm -it --user=1000 -v /home/xps15/.mim-dsg/heni:/home/heni/.heni -v $(TOCK_ROOT_DIRECTORY):$(TOCK_ROOT_DIRECTORY) -w=$(TOCK_ROOT_DIRECTORY) --mount type=bind,source=/dev,target=/dev ghcr.io/mimuw-distributed-systems-group/heni_client:heni heni


### Flashing kernel only

.PHONY: flash
flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).bin
	$(OPENOCD) -f flash-kernel.openocd

$(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).hex: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).bin
	arm-none-eabi-objcopy -I binary -O ihex $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).bin $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).hex

.PHONY: hex
hex: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).hex

### Flashing kernel + an app

$(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-%.elf: $(LIBTOCK_RS_TARGET)/%.tbf $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).elf release
	arm-none-eabi-objcopy --update-section .apps=$(LIBTOCK_RS_TARGET)/$(shell basename $<) $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM).elf $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-$(shell basename -s .tbf $<).elf

.PHONY: app-*-elf
app-%-elf: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-%.elf

$(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-%.hex: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-%.elf
	arm-none-eabi-objcopy -O ihex $< $(<:.elf=.hex)

.PHONY: app-*-hex
app-%-hex: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-%.hex

.PHONY: app-*-flash
app-%-flash: $(TOCK_ROOT_DIRECTORY)target/$(TARGET)/release/$(PLATFORM)-%.hex
	$(HENI) node program dk $<
