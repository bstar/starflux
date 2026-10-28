#!/usr/bin/env python3
"""Decode private acceptance-test video; report the canvas cursor, not a local overlay."""
import base64
import json
import sys
import time

import gi

gi.require_version('Gst', '1.0')
gi.require_version('GstVideo', '1.0')
from gi.repository import Gst, GstVideo

Gst.init(None)
pipeline = None


def open_decoder(message):
    pipeline = Gst.parse_launch('appsrc name=input is-live=true format=time ! h264parse ! avdec_h264 max-threads=1 ! videoconvert ! video/x-raw,format=RGB ! appsink name=output sync=false')
    source = pipeline.get_by_name('input')
    source.set_property('caps', Gst.Caps.from_string('video/x-h264,stream-format=byte-stream,alignment=au'))
    sink = pipeline.get_by_name('output')

    def decoded(sink):
        sample = sink.emit('pull-sample')
        info = GstVideo.VideoInfo.new_from_caps(sample.get_caps())
        data = sample.get_buffer().extract_dup(0, sample.get_buffer().get_size())
        row = data[info.stride[0] * (info.height // 2):][:info.width * 3]
        red = [x for x in range(info.width) if row[x * 3] > 180 and row[x * 3 + 1] < 90 and row[x * 3 + 2] < 90]
        print(json.dumps(dict(session=message['session'], generation=message['generation'], cursor_x=min(red) if red else None, width=info.width, height=info.height, decoded_ns=time.monotonic_ns())), flush=True)
        return Gst.FlowReturn.OK

    sink.set_property('emit-signals', True)
    sink.connect('new-sample', decoded)
    pipeline.set_state(Gst.State.PLAYING)
    return pipeline, source


try:
    for line in sys.stdin:
        message = json.loads(line)
        if message['kind'] == 'canvas_video_open':
            if pipeline:
                pipeline.set_state(Gst.State.NULL)
            pipeline, source = open_decoder(message)
        elif message['kind'] == 'canvas_video_packet':
            data = base64.b64decode(message['data'])
            buffer = Gst.Buffer.new_allocate(None, len(data), None)
            buffer.fill(0, data)
            if source.emit('push-buffer', buffer) != Gst.FlowReturn.OK:
                raise RuntimeError('Private cursor decoder rejected a packet')
finally:
    if pipeline:
        pipeline.set_state(Gst.State.NULL)
